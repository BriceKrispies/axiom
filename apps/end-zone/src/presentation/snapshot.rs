//! The immutable per-tick presentation snapshot: everything player/football/
//! field rendering, the camera director, juice, and the debug overlay may
//! read. Captured by value from the simulation once per tick — presentation
//! never holds a mutable handle to simulation internals.

use axiom::prelude::Vec3;

use crate::ai::engagement::{EngagementState, RushLane};
use crate::ai::{
    AssignmentKind, AssignmentOverride, DefensiveDirective, PlayerIntent, Responsibility,
    RoleState, TacticalMode,
};
use crate::attempt::AttemptStep;
use crate::events::PlayEndReason;
use crate::football::{BallSim, BallSituation, BallState, FlightInfo};
use crate::identity::{PlayerId, TeamId};
use crate::player::AnimState;
use crate::state::{PlayPhase, SimState};

/// One player's render-relevant view.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PlayerView {
    pub id: PlayerId,
    pub team: TeamId,
    pub jersey: u8,
    pub pos: Vec3,
    pub vel: Vec3,
    pub facing: f32,
    pub anim: AnimState,
    pub anim_ticks: u32,
    pub speed: f32,
    pub body_radius: f32,
    pub catch_radius: f32,
    pub role: RoleState,
    pub intent: PlayerIntent,
    /// The coordinated pursuit responsibility this tick (AI debug view).
    pub responsibility: Responsibility,
    /// The committed-action debug reason, if committed.
    pub action_reason: Option<&'static str>,
    /// Ticks of committed action left before a free switch.
    pub commit_ticks: u32,
    /// The line-engagement state + advantage + rush lane, if engaged as a
    /// blocker (AI debug view).
    pub engagement_state: Option<EngagementState>,
    pub engagement_advantage: f32,
    pub rush_lane: Option<RushLane>,
    /// The overseer's assignment override on this defender (AI debug view).
    pub def_override: AssignmentOverride,
}

/// One offensive route drawn as pre-snap chalk on the field: the path in world
/// space from the receiver's alignment through his waypoints. The primary read
/// is highlighted; the field renderer dots the line like a chalkboard.
#[derive(Debug, Clone, PartialEq)]
pub struct ChalkRoute {
    pub points: Vec<Vec3>,
    pub primary: bool,
}

/// The wind-up preview: where the ball would go if released right now.
#[derive(Debug, Clone, PartialEq)]
pub struct ThrowPreview {
    /// Sampled arc through the air, release → landing.
    pub arc: Vec<Vec3>,
    /// Where the ball comes down.
    pub landing: Vec3,
    /// How far the wind-up has charged, `0..=1`.
    pub charge: f32,
}

/// How many points the previewed arc is sampled at.
pub const ARC_SAMPLES: usize = 18;

/// The immutable snapshot. Same snapshot + same effect state → same scene
/// submission.
#[derive(Debug, Clone, PartialEq)]
pub struct PresentationSnapshot {
    pub tick: u64,
    pub seed: u64,
    pub phase: PlayPhase,
    pub end_reason: Option<PlayEndReason>,
    pub possession: Option<PlayerId>,
    pub quarterback: PlayerId,
    pub ball: BallSim,
    pub flight: Option<FlightInfo>,
    pub players: Vec<PlayerView>,
    pub line_of_scrimmage_z: f32,
    /// `+1` when the offense drives toward `+Z`, else `-1`.
    pub drive_sign: f32,
    pub gravity: f32,
    pub fault: Option<&'static str>,
    /// The football situation the AI derived this tick (AI debug view).
    pub ball_situation: BallSituation,
    /// The overseer's active directive (AI debug view).
    pub directive: DefensiveDirective,
    /// The overseer's previous mode + last transition reason (AI debug view).
    pub overseer_prev_mode: TacticalMode,
    pub overseer_transition_reason: &'static str,
    /// The top rejected tactical alternative + its score (AI debug view).
    pub overseer_rejected: (TacticalMode, f32),
    /// The attempt loop's state, when this is a real session (the ambient menu
    /// showcase leaves it `None`). Presentation reads the decision phase, the
    /// live read, and the last result from here — never from the loop itself.
    pub attempt: Option<AttemptStep>,
    /// World `Z` of the bright field marker: the line the current attempt
    /// snapped from, so a gain or a loss is legible against it at a glance.
    pub spot_marker_z: Option<f32>,
    /// The live wind-up preview: the arc the ball would fly on if the throw
    /// were released THIS tick, and where it would come down. Present only
    /// while a read is held. This is the whole feedback loop for a charged
    /// pass — without it "throw harder" is a guess.
    pub throw_preview: Option<ThrowPreview>,
    /// The receivers the quarterback can throw to right now — everyone inside
    /// his throwing cone, nearest his centre line first. The scene draws a ring
    /// at each one's feet; the pass would go to the first.
    pub throwable: Vec<PlayerId>,
    /// The selected play's offensive routes drawn as pre-snap field chalk. Empty
    /// except before the snap, so the chalk shows only while the offense is set.
    pub pre_snap_routes: Vec<ChalkRoute>,
}

impl PresentationSnapshot {
    /// The player view for `id`.
    pub fn player(&self, id: PlayerId) -> &PlayerView {
        &self.players[id.index()]
    }

    /// The current carrier's view, if the ball is held.
    pub fn carrier(&self) -> Option<&PlayerView> {
        self.ball.carrier().map(|id| self.player(id))
    }
}

/// The selected play's offensive routes as pre-snap chalk: each route runner's
/// path from his alignment through his world waypoints, the primary read (the
/// highest-slot live route) flagged. Empty unless the offense is set pre-snap.
fn pre_snap_chalk(sim: &SimState) -> Vec<ChalkRoute> {
    if sim.phase != PlayPhase::PreSnap {
        return Vec::new();
    }
    let offense = sim.play.possession;
    let is_live_route = |a: &AssignmentKind| matches!(a, AssignmentKind::Route { decoy: false });
    let primary = sim
        .assignments
        .iter()
        .enumerate()
        .filter(|(i, a)| sim.players[*i].team == offense && is_live_route(&a.kind))
        .map(|(i, _)| i)
        .max();
    sim.assignments
        .iter()
        .enumerate()
        .filter(|(i, a)| {
            sim.players[*i].team == offense
                && matches!(a.kind, AssignmentKind::Route { .. })
                && !a.route.is_empty()
        })
        .map(|(i, a)| {
            let mut points = Vec::with_capacity(a.route.len() + 1);
            points.push(sim.players[i].pos);
            points.extend(a.route.iter().copied());
            ChalkRoute {
                points,
                primary: Some(i) == primary,
            }
        })
        .collect()
}

/// The live wind-up preview, if a read is being held.
///
/// Solved with exactly the functions the release itself uses, so what the
/// player is shown is not an approximation of the throw — it IS the throw,
/// evaluated one tick early.
fn throw_preview(sim: &SimState) -> Option<ThrowPreview> {
    let target = sim.charge_target()?;
    let qb = sim.players.get(sim.quarterback.index())?;
    let receiver = sim.players.get(target.index())?;
    let charge = sim.charge_ratio();
    let release = crate::football::carry_socket(qb.pos, qb.facing, AnimState::Throw);
    // The SAME solve the release runs, one tick early — so the arc the player
    // is shown is not a model of the throw, it is the throw.
    let (_, velocity) = crate::football::flight::aim_and_velocity(
        release,
        receiver.pos,
        receiver.vel,
        charge,
        sim.tuning.gravity,
        &sim.tuning,
    );
    let ground = crate::football::catch_point(Vec3::ZERO).y;
    let (landing, _) =
        crate::football::flight::predict_landing(release, velocity, sim.tuning.gravity, ground);
    Some(ThrowPreview {
        arc: crate::football::flight::arc_samples(
            release,
            velocity,
            sim.tuning.gravity,
            ground,
            ARC_SAMPLES,
        ),
        landing,
        charge,
    })
}

/// Capture this tick's snapshot from the simulation (read-only).
pub fn capture(sim: &SimState) -> PresentationSnapshot {
    let players = sim
        .players
        .iter()
        .enumerate()
        .map(|(index, p)| PlayerView {
            id: p.id,
            team: p.team,
            jersey: p.jersey,
            pos: p.pos,
            vel: p.vel,
            facing: p.facing,
            anim: p.anim,
            anim_ticks: p.anim_ticks,
            speed: p.speed(),
            body_radius: p.archetype.body_radius,
            catch_radius: p.archetype.catch_radius,
            role: sim.roles[index],
            intent: sim.intents[index],
            responsibility: sim.responsibility(p.id),
            action_reason: sim.commitment_reason(p.id),
            commit_ticks: sim.commitment_ticks_left(p.id),
            engagement_state: sim.engagement(p.id).map(|e| e.state),
            engagement_advantage: sim.engagement(p.id).map(|e| e.advantage).unwrap_or(0.0),
            rush_lane: sim.engagement(p.id).map(|e| e.lane),
            def_override: sim.directive().override_for(p.id),
        })
        .collect();
    let (overseer_prev_mode, overseer_transition_reason) = sim.overseer_transition();
    PresentationSnapshot {
        tick: sim.tick,
        seed: sim.seed,
        phase: sim.phase,
        end_reason: sim.end_reason,
        possession: sim.possession,
        quarterback: sim.quarterback,
        ball: sim.ball,
        flight: match sim.ball.state {
            BallState::Airborne { flight } => Some(flight),
            _ => None,
        },
        players,
        line_of_scrimmage_z: sim.frame.line_of_scrimmage_z,
        drive_sign: sim.frame.direction.sign(),
        gravity: sim.tuning.gravity,
        fault: sim.fault(),
        ball_situation: sim.ball_situation(),
        directive: sim.directive(),
        overseer_prev_mode,
        overseer_transition_reason,
        overseer_rejected: sim.overseer_rejected(),
        // The run layer fills the attempt view in; the raw sim capture is
        // loop-agnostic. The spot marker always tracks the live line of
        // scrimmage, which is the attempt's own start line.
        attempt: None,
        throwable: sim.throwable.clone(),
        spot_marker_z: Some(sim.frame.line_of_scrimmage_z),
        throw_preview: throw_preview(sim),
        pre_snap_routes: pre_snap_chalk(sim),
    }
}
