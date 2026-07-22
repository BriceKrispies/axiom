//! Defensive coordination: a stateless, deterministic geometric pass that hands
//! each defender one pursuit responsibility so the team keeps basic football
//! shape — one primary tackler, one outside-contain, one cutback defender, deep
//! help, and (on a thrown ball) an interceptor, a contest, and a post-catch
//! tackle angle. It writes into the shared [`PlayPerception`]; each defender's
//! candidate scoring then reads its responsibility and applies duplication
//! penalties. This is **not** an overseer — it is re-derived from geometry every
//! tick and only gives each individual a responsibility to weigh.

use axiom::prelude::Vec3;

use crate::data::BehaviorTuning;
use crate::identity::PlayerId;
use crate::player::PlayerSim;

use super::perception::{PlayPerception, Responsibility};

/// Assign every defender a responsibility for this tick.
pub fn assign_responsibilities(
    per: &mut PlayPerception,
    players: &[PlayerSim],
    tuning: &BehaviorTuning,
) {
    let defenders: Vec<PlayerId> = players
        .iter()
        .filter(|p| p.team != per.offense_team && p.anim.can_act())
        .map(|p| p.id)
        .collect();
    if defenders.is_empty() {
        return;
    }

    if per.situation.ball_in_air() {
        assign_airborne(per, players, &defenders, tuning);
    } else if let Some(runner) = per.ground_threat {
        assign_ground(per, players, &defenders, players[runner.index()].pos, players[runner.index()].vel);
    } else if per.situation.is_loose() {
        // A loose ball: the nearest defender is the primary recoverer.
        assign_ground(per, players, &defenders, per.ball_pos, Vec3::ZERO);
    }
}

/// Rally the defense to a live ground carrier (or a loose ball at `threat`).
fn assign_ground(
    per: &mut PlayPerception,
    players: &[PlayerSim],
    defenders: &[PlayerId],
    threat: Vec3,
    threat_vel: Vec3,
) {
    let sign = per.drive_sign;
    let escape = if threat_vel.x.abs() > 0.5 {
        threat_vel.x.signum()
    } else {
        threat.x.signum()
    };
    let pos = |id: PlayerId| players[id.index()].pos;

    // Primary: the nearest viable defender attacks the carrier.
    if let Some(id) = pick_best(per, defenders, |d| Some(-flat_dist(pos(d), threat))) {
        per.responsibilities[id.index()] = Responsibility::PrimaryTackler;
    }
    // Deep help: whoever is furthest downfield preserves touchdown prevention.
    if let Some(id) = pick_best(per, defenders, |d| Some((pos(d).z - threat.z) * sign)) {
        per.responsibilities[id.index()] = Responsibility::DeepHelp;
    }
    // Outside contain: the defender furthest to the carrier's escape side.
    if let Some(id) = pick_best(per, defenders, |d| Some(pos(d).x * escape)) {
        per.responsibilities[id.index()] = Responsibility::OutsideContain;
    }
    // Cutback: the defender best positioned on the inside.
    if let Some(id) = pick_best(per, defenders, |d| Some(-pos(d).x * escape)) {
        per.responsibilities[id.index()] = Responsibility::Cutback;
    }
}

/// Assign responsibilities against a ball in flight (spec §7).
fn assign_airborne(
    per: &mut PlayPerception,
    players: &[PlayerSim],
    defenders: &[PlayerId],
    tuning: &BehaviorTuning,
) {
    let (Some(cp), Some(eta)) = (per.catch_point, per.eta_tick) else {
        return;
    };
    let pos = |id: PlayerId| players[id.index()].pos;
    let can_arrive = |id: PlayerId| -> bool {
        let reach = arrival_ticks(pos(id), cp, players[id.index()].archetype.max_speed);
        per.tick + reach <= eta + u64::from(tuning.contest_window_ticks)
    };

    // Interceptor: of the defenders who can beat the ball there, the nearest.
    if let Some(id) = pick_best(per, defenders, |d| {
        can_arrive(d).then(|| -flat_dist(pos(d), cp))
    }) {
        per.responsibilities[id.index()] = Responsibility::Intercept;
    }
    // Contest: the next nearest defender to the catch point.
    if let Some(id) = pick_best(per, defenders, |d| Some(-flat_dist(pos(d), cp))) {
        per.responsibilities[id.index()] = Responsibility::ContestCatch;
    }
    // Tackle angle: whoever is nearest the intended receiver preps the tackle.
    if let Some(receiver) = per.intended_receiver {
        let recv_pos = players[receiver.index()].pos;
        if let Some(id) = pick_best(per, defenders, |d| Some(-flat_dist(pos(d), recv_pos))) {
            per.responsibilities[id.index()] = Responsibility::TackleAngle;
        }
    }
}

/// The highest-`key` unassigned defender (ties broken by ascending id, since
/// `defenders` is in id order and we keep the first strict maximum). A `None`
/// key excludes the defender from this pick.
fn pick_best(
    per: &PlayPerception,
    defenders: &[PlayerId],
    key: impl Fn(PlayerId) -> Option<f32>,
) -> Option<PlayerId> {
    let mut best: Option<(PlayerId, f32)> = None;
    for &id in defenders {
        if per.responsibilities[id.index()] != Responsibility::None {
            continue;
        }
        if let Some(k) = key(id) {
            let take = best.map(|(_, bk)| k > bk).unwrap_or(true);
            if take {
                best = Some((id, k));
            }
        }
    }
    best.map(|(id, _)| id)
}

/// Ticks for a mover at `max_speed` to cover the flat distance to `target`.
fn arrival_ticks(from: Vec3, target: Vec3, max_speed: f32) -> u64 {
    let seconds = flat_dist(from, target) / max_speed.max(0.1);
    (seconds * 60.0).round().max(0.0) as u64
}

fn flat_dist(a: Vec3, b: Vec3) -> f32 {
    Vec3::new(a.x - b.x, 0.0, a.z - b.z).length()
}
