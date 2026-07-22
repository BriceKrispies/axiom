//! The immutable per-tick presentation snapshot: everything player/football/
//! field rendering, the camera director, juice, and the debug overlay may
//! read. Captured by value from the simulation once per tick — presentation
//! never holds a mutable handle to simulation internals.

use axiom::prelude::Vec3;

use crate::ai::engagement::{EngagementState, RushLane};
use crate::ai::{
    AssignmentOverride, DefensiveDirective, PlayerIntent, Responsibility, RoleState, TacticalMode,
};
use crate::drive::DriveState;
use crate::events::PlayEndReason;
use crate::football::{BallSim, BallState, BallSituation, FlightInfo};
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
    /// The authoritative drive state, when this is a real score-attack run
    /// (the ambient menu showcase leaves it `None`).
    pub drive: Option<DriveState>,
    /// World `Z` of the line to gain, when a drive is active (the field marker).
    pub to_gain_z: Option<f32>,
    /// The receivers the quarterback can throw to right now — everyone inside
    /// his throwing cone, nearest his centre line first. The scene draws a ring
    /// at each one's feet; the pass would go to the first.
    pub throwable: Vec<PlayerId>,
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
        // The run layer fills these in for a real drive; the raw sim capture
        // is drive-agnostic.
        drive: None,
        throwable: sim.throwable.clone(),
        to_gain_z: None,
    }
}
