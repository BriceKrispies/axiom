//! The contact framework: blocking engagements, tackle evaluation, and the
//! controlled fall (stumble → airborne → ground impact → recovery). Outcomes
//! are deterministic and authoritative; there is no ragdoll — falls are
//! procedural pose states driven by the fixed tick.

use axiom::prelude::Vec3;

use crate::ai::PlayerIntent;
use crate::collision_rig::CollisionRig;
use crate::data::BehaviorTuning;
use crate::identity::PlayerId;

use super::tackle::{self, TackleContest};
use super::{AnimState, PlayerSim};

/// Ticks a stumble lasts before the trip completes.
const STUMBLE_TICKS: u32 = 10;
/// Ticks the ground-impact pose holds before recovery starts.
const GROUND_TICKS: u32 = 16;

/// What one tick's tackling produced: the hit that brought the carrier down (if
/// any), and every attempt he shed getting there.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct TackleResolution {
    /// The tackle that landed, with the contest that decided it.
    pub landed: Option<TackleOutcome>,
    /// Attempts the carrier survived, in tackler id order.
    pub shed: Vec<TackleContest>,
}

impl TackleResolution {
    /// Whether anybody got a hand on the carrier at all this tick.
    pub fn any_contact(&self) -> bool {
        self.landed.is_some() || !self.shed.is_empty()
    }
}

/// A tackle that landed this tick.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TackleOutcome {
    pub tackler: PlayerId,
    pub target: PlayerId,
    pub contact_point: Vec3,
    pub contact_direction: Vec3,
    pub relative_speed: f32,
    pub strength: f32,
    pub target_airborne: bool,
    /// The contest itself, so the reason is inspectable downstream.
    pub contest: TackleContest,
}

/// Tackle evaluation: every defender who reaches the carrier this tick gets a
/// **contest**, in id order, until one of them wins it.
///
/// It used to be that reaching him *was* winning: the first man in range with
/// any closing speed above a floor put him on the turf, every time. Now reaching
/// him only earns the attempt (see [`super::tackle`]), and a man who has run the
/// carrier down but has no closing speed left to hit him with gets shed. The
/// hits and the sheds are both authoritative and deterministic — impulse against
/// resistance, no ragdoll and no roll.
pub fn resolve_tackle(
    players: &mut [PlayerSim],
    intents: &[PlayerIntent],
    carrier: Option<PlayerId>,
    carrier_charging: bool,
    tuning: &BehaviorTuning,
    collision: &CollisionRig,
) -> TackleResolution {
    let mut resolution = TackleResolution::default();
    let Some(carrier) = carrier else {
        return resolution;
    };
    // A back mid-charge is running THROUGH people: nobody brings him down for
    // the length of the window. The defenders he meets are dealt with by
    // `runback::stage::carry_charge`, which knocks them aside — so this is not
    // "nothing happens", it is "the collision goes the other way".
    if carrier_charging || !players[carrier.index()].anim.can_act() {
        return resolution;
    }
    for index in 0..players.len() {
        // Either a standing chaser holding a `Tackle` intent, or a committed
        // diver mid-lunge (whose intent has already lapsed — the dive is the
        // commitment) can attempt the hit here.
        let diving = players[index].anim == AnimState::Dive;
        let standing = matches!(
            intents[index],
            PlayerIntent::Tackle { target, .. } if target == carrier
        ) && players[index].anim.can_act();
        if !(diving || standing) {
            continue;
        }
        let tackler_pos = players[index].pos;
        let carrier_sim = &players[carrier.index()];
        let to_carrier = Vec3::new(
            carrier_sim.pos.x - tackler_pos.x,
            0.0,
            carrier_sim.pos.z - tackler_pos.z,
        );
        let distance = to_carrier.length();
        // A dive reaches only on real body contact from the collision world (arc
        // height included); a standing tackle keeps its horizontal arm-reach —
        // but only up to the height a man on his feet can actually reach.
        //
        // That height gate is the fix for a defect the horizontal-only test had
        // from the start: a whiffed dive sailing clean over the carrier still
        // measured "in range" and landed a phantom tackle, which ended the play
        // and shook the camera for a hit that never happened. It is also what
        // makes the running back's leap a real answer rather than an animation
        // — a carrier genuinely above a defender's arms cannot be brought down
        // by them, and that is one rule, written once, for both cases.
        let within_reach = carrier_sim.pos.y <= tuning.tackle_reach_height;
        let reached = match diving {
            true => collision.in_contact(players[index].id, carrier),
            false => distance <= tuning.tackle_range && within_reach,
        };
        if !reached {
            continue;
        }
        let relative = players[index].vel.subtract(carrier_sim.vel);
        let relative_speed = relative.length() + players[index].speed() * 0.25;
        // A diver is already airborne and past the point of no return — no
        // minimum-closing-speed gate; a standing attempt still needs pop to be
        // worth making at all.
        if !diving && relative_speed < tuning.tackle_min_closing_speed {
            continue;
        }

        let contest = tackle::contest(&players[index], &players[carrier.index()], diving, tuning);
        if !contest.landed {
            tackle::apply_shed(players, &contest, tuning);
            resolution.shed.push(contest);
            // The next man in id order still gets his attempt this tick, against
            // a carrier who is now slower and less balanced — which is exactly
            // how a swarm is supposed to work.
            continue;
        }
        let contact_point = players[carrier.index()].pos.add(Vec3::new(0.0, 1.0, 0.0));
        let strength = contest.strength(tuning);
        let target_airborne = tackle::apply_landed(players, &contest, tuning);
        resolution.landed = Some(TackleOutcome {
            tackler: contest.tackler,
            target: carrier,
            contact_point,
            contact_direction: contest.direction,
            relative_speed,
            strength,
            target_airborne,
            contest,
        });
        return resolution;
    }
    resolution
}

/// Commit diving tackles: a chaser holding a `Tackle` intent whose carrier is
/// just beyond standing range, closing fast, and actually escaping (moving)
/// leaves their feet — a ballistic forward lunge. The dive is landed later by
/// [`resolve_tackle`]'s dive path, or whiffed into the turf by [`advance_falls`].
/// Called only when no standing tackle landed this tick.
pub fn commit_dives(
    players: &mut [PlayerSim],
    intents: &[PlayerIntent],
    carrier: Option<PlayerId>,
    tuning: &BehaviorTuning,
) {
    let Some(carrier) = carrier else {
        return;
    };
    if !players[carrier.index()].anim.can_act() {
        return;
    }
    for index in 0..players.len() {
        let PlayerIntent::Tackle { target, .. } = intents[index] else {
            continue;
        };
        if target != carrier || !players[index].anim.can_act() {
            continue;
        }
        let tackler_pos = players[index].pos;
        let carrier_sim = &players[carrier.index()];
        let to = Vec3::new(
            carrier_sim.pos.x - tackler_pos.x,
            0.0,
            carrier_sim.pos.z - tackler_pos.z,
        );
        let distance = to.length();
        let in_window =
            distance > tuning.tackle_range && distance <= tuning.tackle_range * tuning.dive_window;
        let relative = players[index].vel.subtract(carrier_sim.vel);
        let closing = relative.length() + players[index].speed() * 0.25;
        let escaping = carrier_sim.speed() >= tuning.dive_carrier_min_speed;
        // A flat-out runner matched for speed is WRAPPED standing, not dived at.
        // A committed dive is ballistic: it whiffs against a juke and, having left
        // its feet, the diver is spent and removed from the play. So when the
        // carrier is at a full sprint (>= 85% of its own top speed) AND this
        // tackler is fast enough to stay stride-for-stride (its top speed meets
        // the carrier's current speed), the tackler keeps its feet and lets the
        // standing tracking-tackle in `resolve_tackle` finish the play — a
        // juke-proof run-down. The gate keys on the carrier being FLAT-OUT so a
        // slower carrier (a scrambling QB) is still dived at, and the fast-chaser-
        // on-slow-carrier dive path stays intact.
        let carrier_flat_out = carrier_sim.speed() >= 0.85 * carrier_sim.archetype.max_speed;
        let can_stay_with = players[index].archetype.max_speed >= carrier_sim.speed();
        let wrap_instead = carrier_flat_out && can_stay_with;
        if in_window
            && closing >= tuning.dive_min_closing_speed
            && escaping
            && !wrap_instead
            && distance > 1.0e-4
        {
            let dir = to.mul_scalar(1.0 / distance);
            let diver = &mut players[index];
            diver.facing = dir.x.atan2(dir.z);
            diver.vel = dir.mul_scalar(tuning.dive_launch_forward);
            diver.vertical_vel = tuning.dive_launch_up;
            diver.impact_strength = tuning.dive_whiff_impact;
            diver.set_anim(AnimState::Dive);
        }
    }
}

/// Advance controlled falls: airborne arcs under gravity, stumbles that trip,
/// the ground-impact hold, and recovery back to standing. Returns the players
/// who hit the turf this tick (with their stored impact strengths).
pub fn advance_falls(
    players: &mut [PlayerSim],
    tuning: &BehaviorTuning,
    dt: f32,
) -> Vec<(PlayerId, f32)> {
    let mut impacts = Vec::new();
    for player in players.iter_mut() {
        match player.anim {
            AnimState::Dive => {
                // Ballistic forward lunge under gravity; a landed dive is
                // grounded by `resolve_tackle` before this runs, so reaching
                // the turf here is a whiff.
                player.vertical_vel -= tuning.gravity * dt;
                player.pos = Vec3::new(
                    player.pos.x + player.vel.x * dt,
                    (player.pos.y + player.vertical_vel * dt).max(0.0),
                    player.pos.z + player.vel.z * dt,
                );
                player.vel = player.vel.mul_scalar(0.99);
                if player.pos.y <= 0.0 && player.vertical_vel < 0.0 {
                    player.pos = Vec3::new(player.pos.x, 0.0, player.pos.z);
                    player.vertical_vel = 0.0;
                    player.vel = player.vel.mul_scalar(0.15);
                    player.set_anim(AnimState::GroundImpact);
                    impacts.push((player.id, player.impact_strength));
                }
            }
            AnimState::AirborneFall => {
                player.vertical_vel -= tuning.gravity * dt;
                player.pos = Vec3::new(
                    player.pos.x + player.vel.x * dt,
                    (player.pos.y + player.vertical_vel * dt).max(0.0),
                    player.pos.z + player.vel.z * dt,
                );
                player.vel = player.vel.mul_scalar(0.985);
                if player.pos.y <= 0.0 && player.vertical_vel < 0.0 {
                    player.pos = Vec3::new(player.pos.x, 0.0, player.pos.z);
                    player.vertical_vel = 0.0;
                    player.vel = player.vel.mul_scalar(0.2);
                    player.set_anim(AnimState::GroundImpact);
                    impacts.push((player.id, player.impact_strength));
                }
            }
            // Bounced off a carrier he could not bring down: on his feet, but
            // out of the play for a beat. Without this the shed defender simply
            // re-attempts on the very next tick and nothing was survived.
            AnimState::HitReaction => {
                player.pos = Vec3::new(
                    player.pos.x + player.vel.x * dt,
                    0.0,
                    player.pos.z + player.vel.z * dt,
                );
                player.vel = player.vel.mul_scalar(0.86);
                if player.anim_ticks >= tuning.hit_reaction_ticks {
                    player.set_anim(AnimState::Idle);
                }
            }
            AnimState::Stumble => {
                player.pos = Vec3::new(
                    player.pos.x + player.vel.x * dt,
                    0.0,
                    player.pos.z + player.vel.z * dt,
                );
                player.vel = player.vel.mul_scalar(0.92);
                if player.anim_ticks >= STUMBLE_TICKS {
                    player.vel = player.vel.mul_scalar(0.2);
                    player.set_anim(AnimState::GroundImpact);
                    impacts.push((player.id, player.impact_strength));
                }
            }
            AnimState::GroundImpact => {
                player.vel = player.vel.mul_scalar(0.8);
                if player.anim_ticks >= GROUND_TICKS {
                    player.set_anim(AnimState::Recovery);
                }
            }
            AnimState::Recovery => {
                if player.anim_ticks >= tuning.recovery_ticks {
                    player.balance = 1.0;
                    player.set_anim(AnimState::Idle);
                }
            }
            _ => {}
        }
    }
    impacts
}
