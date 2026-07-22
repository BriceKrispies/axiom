//! The overseer's deterministic team-level read of the whole play — the
//! aggregated evidence a defensive coordinator would see at a glance: pocket
//! integrity and pressure, whether the quarterback is rolling out, the most
//! dangerous receiver by *observable* separation, deep / crossing / sideline
//! threats, and scoring danger. Built only from simulation state available to
//! the players (never queued input, the intended target, or future state).

use axiom::prelude::Vec3;

use crate::football::BallSituation;
use crate::identity::PlayerId;
use crate::player::PlayerSim;

use super::perception::PlayPerception;

/// How the pocket is holding up.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PocketState {
    #[default]
    Stable,
    Compressing,
    Broken,
}

impl PocketState {
    pub fn label(self) -> &'static str {
        match self {
            PocketState::Stable => "stable",
            PocketState::Compressing => "compressing",
            PocketState::Broken => "broken",
        }
    }
}

/// The aggregated read of the play this evaluation.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DefensiveRead {
    pub pocket_state: PocketState,
    /// Distance of the nearest rusher to the quarterback, yards.
    pub pressure_distance: f32,
    /// Ticks since the snap (how long the quarterback has held it).
    pub ticks_since_snap: u32,
    pub qb_rollout: bool,
    /// Quarterback depth behind the line of scrimmage, yards (negative = past it).
    pub qb_depth: f32,
    /// The most dangerous receiver by observable separation + depth, if any.
    pub most_dangerous: Option<PlayerId>,
    pub danger_separation: f32,
    /// Receivers open deep behind the deepest defender.
    pub deep_threats: u32,
    /// Two or more receivers converging across the middle.
    pub crossing: bool,
    /// Net receiver lateral bias `-1..=1` (magnitude past ~0.5 is an overload).
    pub sideline_overload: f32,
    pub touchdown_threat: bool,
    pub first_down_threat: bool,
    /// Defenders not tightly committed to a block or tackle this tick.
    pub free_defenders: u32,
}

/// Build the read from the shared perception and true player state.
pub fn read(per: &PlayPerception, players: &[PlayerSim], snap_tick: u64) -> DefensiveRead {
    let ticks_since_snap = per.tick.saturating_sub(snap_tick) as u32;
    let sign = per.drive_sign;
    let qb = per.qb_pos;
    let defenders: Vec<&PlayerSim> = players
        .iter()
        .filter(|p| p.team != per.offense_team)
        .collect();
    let receivers: Vec<&PlayerSim> = players
        .iter()
        .filter(|p| p.team == per.offense_team && p.id != per.quarterback)
        .collect();

    let pressure_distance = defenders
        .iter()
        .map(|d| flat_dist(d.pos, qb))
        .fold(f32::MAX, f32::min);
    let pocket_state = if pressure_distance < 2.2 {
        PocketState::Broken
    } else if pressure_distance < 4.5 {
        PocketState::Compressing
    } else {
        PocketState::Stable
    };

    let qb_depth = (per.pocket.los_z - qb.z) * sign;
    let qb_rollout = !per.qb_in_pocket
        && (qb.x.abs() > per.pocket.half_width * 0.6 || per.qb_vel.x.abs() > 2.0);

    // The deepest defender's downfield reach (the back edge of coverage).
    let deep_edge = defenders
        .iter()
        .map(|d| (d.pos.z - per.pocket.los_z) * sign)
        .fold(f32::MIN, f32::max);

    // Most dangerous receiver: observable separation from the nearest defender,
    // weighted by how far downfield he is.
    let mut most_dangerous = None;
    let mut danger_separation = 0.0f32;
    let mut deep_threats = 0u32;
    let mut left = 0.0f32;
    let mut right = 0.0f32;
    for r in &receivers {
        let sep = defenders
            .iter()
            .map(|d| flat_dist(d.pos, r.pos))
            .fold(f32::MAX, f32::min);
        let depth = (r.pos.z - per.pocket.los_z) * sign;
        let threat = sep + 0.15 * depth.max(0.0);
        if most_dangerous.is_none() || threat > danger_separation {
            danger_separation = threat;
            most_dangerous = Some(r.id);
        }
        if depth > deep_edge - 1.0 && depth > 8.0 {
            deep_threats += 1;
        }
        (r.pos.x < 0.0).then(|| left += 1.0).unwrap_or_else(|| right += 1.0);
    }
    // Only report a *separation* number, not the depth-weighted threat, so the
    // read stays interpretable.
    let danger_separation = most_dangerous
        .map(|id| {
            defenders
                .iter()
                .map(|d| flat_dist(d.pos, players[id.index()].pos))
                .fold(f32::MAX, f32::min)
        })
        .unwrap_or(0.0);

    let total = (left + right).max(1.0);
    let sideline_overload = (right - left) / total;
    let crossing = crossing_middle(&receivers);

    let goal_gap = (per.end_zone.z - qb.z).abs();
    let touchdown_threat = goal_gap < 22.0 || (per.situation.ball_in_air() && catch_near_goal(per));
    let first_down_threat = goal_gap < 35.0;
    let free_defenders = defenders
        .iter()
        .filter(|d| d.anim.can_act() && d.speed() < 6.0)
        .count() as u32;

    DefensiveRead {
        pocket_state,
        pressure_distance,
        ticks_since_snap,
        qb_rollout,
        qb_depth,
        most_dangerous,
        danger_separation,
        deep_threats,
        crossing,
        sideline_overload,
        touchdown_threat,
        first_down_threat,
        free_defenders,
    }
}

/// Two or more receivers heading across the centre from opposite sides.
fn crossing_middle(receivers: &[&PlayerSim]) -> bool {
    let inward = receivers
        .iter()
        .filter(|r| r.pos.x.abs() < 8.0 && r.vel.x * r.pos.x < -0.2)
        .count();
    inward >= 2
}

/// Whether a live pass's catch point sits near the attacked goal.
fn catch_near_goal(per: &PlayPerception) -> bool {
    per.catch_point
        .map(|cp| (per.end_zone.z - cp.z).abs() < 18.0)
        .unwrap_or(false)
}

fn flat_dist(a: Vec3, b: Vec3) -> f32 {
    Vec3::new(a.x - b.x, 0.0, a.z - b.z).length()
}

/// Whether the situation is a pre-throw dropback the coverage modes apply to.
pub fn is_dropback(situation: BallSituation) -> bool {
    matches!(situation, BallSituation::HeldByQb | BallSituation::ThrowWindup)
}
