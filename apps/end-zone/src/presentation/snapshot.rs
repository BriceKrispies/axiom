//! The immutable per-tick presentation snapshot: everything player/football/
//! field rendering, the camera director, juice, and the debug overlay may
//! read. Captured by value from the simulation once per tick — presentation
//! never holds a mutable handle to simulation internals.

use axiom::prelude::Vec3;

use crate::ai::{PlayerIntent, RoleState};
use crate::events::PlayEndReason;
use crate::football::{BallSim, BallState, FlightInfo};
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
    pub stride: f32,
    pub speed: f32,
    pub body_radius: f32,
    pub catch_radius: f32,
    pub role: RoleState,
    pub intent: PlayerIntent,
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
            stride: p.stride,
            speed: p.speed(),
            body_radius: p.archetype.body_radius,
            catch_radius: p.archetype.catch_radius,
            role: sim.roles[index],
            intent: sim.intents[index],
        })
        .collect();
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
    }
}
