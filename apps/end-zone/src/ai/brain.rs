//! The per-player brain dispatch: role state, the shared decision context,
//! and the delayed-perception buffer defenders read (configurable reaction
//! delay instead of instant mirroring).

use axiom::prelude::Vec3;

use crate::config::PLAYER_COUNT;
use crate::data::BehaviorTuning;
use crate::football::BallSim;
use crate::identity::PlayerId;
use crate::player::PlayerSim;

use super::action::{Priority, ScoredAction};
use super::assignment::{AssignmentKind, ResolvedAssignment};
use super::commitment::{self, Commitment};
use super::engagement::EngagementLink;
use super::perception::PlayPerception;
use super::{defense, offense, PlayerIntent};

/// How many ticks of history perception keeps (bounds every reaction delay).
pub const PERCEPTION_RING: usize = 32;

/// One remembered tick of world state.
#[derive(Debug, Clone, Copy)]
pub struct PerceptionFrame {
    pub positions: [Vec3; PLAYER_COUNT],
    pub velocities: [Vec3; PLAYER_COUNT],
    pub ball_pos: Vec3,
    pub ball_airborne: bool,
    pub ball_target: Vec3,
    pub carrier: Option<PlayerId>,
}

impl PerceptionFrame {
    fn empty() -> Self {
        PerceptionFrame {
            positions: [Vec3::ZERO; PLAYER_COUNT],
            velocities: [Vec3::ZERO; PLAYER_COUNT],
            ball_pos: Vec3::ZERO,
            ball_airborne: false,
            ball_target: Vec3::ZERO,
            carrier: None,
        }
    }
}

/// A bounded ring of [`PerceptionFrame`]s. `sample(delay)` returns the world
/// as it was `delay` ticks ago (clamped to the oldest remembered frame).
#[derive(Debug)]
pub struct Perception {
    ring: Vec<PerceptionFrame>,
    filled: usize,
    head: usize,
}

impl Perception {
    pub fn new() -> Self {
        Perception {
            ring: vec![PerceptionFrame::empty(); PERCEPTION_RING],
            filled: 0,
            head: 0,
        }
    }

    /// Record this tick's frame.
    pub fn push(&mut self, frame: PerceptionFrame) {
        self.head = (self.head + 1) % PERCEPTION_RING;
        self.ring[self.head] = frame;
        self.filled = (self.filled + 1).min(PERCEPTION_RING);
    }

    /// The frame `delay` ticks ago (clamped to what is remembered).
    pub fn sample(&self, delay: u32) -> &PerceptionFrame {
        let clamped = (delay as usize).min(self.filled.saturating_sub(1));
        let index = (self.head + PERCEPTION_RING - clamped) % PERCEPTION_RING;
        &self.ring[index]
    }
}

impl Default for Perception {
    fn default() -> Self {
        Perception::new()
    }
}

/// Per-player role state (small state machines, mutated only by `decide`).
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum RoleState {
    /// Pre-snap set.
    Waiting,
    /// Quarterback: dropping to the drop point.
    QbDrop,
    /// Quarterback: settled, scanning.
    QbScan,
    /// Quarterback: winding up since `since`; the sim releases the ball.
    QbWindup { since: u64 },
    /// Quarterback: ball is out.
    QbDone,
    /// Running a route, heading to waypoint `index`.
    Route { index: usize },
    /// Route finished; working back toward the passer's view.
    RouteDone,
    /// Settling under a thrown ball.
    CatchWork,
    /// Blocking work.
    Blocking,
    /// Carrying the football.
    Carrying,
    /// Defensive work under the current assignment.
    Defending,
    /// Chasing the ball carrier.
    Pursuing,
    /// Knocked down / recovering (movement handled by the controller).
    Down,
}

/// Everything a brain may read. The *shared* play facts come from
/// [`PlayPerception`] (so the team reacts to one play); the *delayed* opponent
/// geometry a defender chases comes from the [`Perception`] ring at his own
/// reaction delay; offense reads true [`players`](Self::players) state.
#[derive(Debug)]
pub struct BrainCtx<'a> {
    pub tick: u64,
    /// Whether the play is live (post-snap, pre-whistle).
    pub live: bool,
    pub tuning: &'a BehaviorTuning,
    pub ball: &'a BallSim,
    pub possession: Option<PlayerId>,
    pub players: &'a [PlayerSim],
    pub perception: &'a Perception,
    /// The shared, read-only play model every brain keys off this tick.
    pub per: &'a PlayPerception,
    /// Last tick's line engagements (read-only) — the rush side checks whether
    /// it has shed its block.
    pub engagements: &'a [EngagementLink],
    /// The play's quarterback (so coverage can tell a passer from a runner).
    pub quarterback: PlayerId,
    /// Where a carrier should run (center of the opponent end zone).
    pub end_zone_target: Vec3,
    /// The showcase controller ordered the quarterback to throw.
    pub throw_commanded: bool,
}

/// Decide one player's intent for this tick: gather this player's scored
/// candidate actions (role-specific, but scored on one shared priority scale),
/// then arbitrate under commitment locking. Downed players recover and drop any
/// commitment. A user-controlled ball-holder takes the top action but holds no
/// AI commitment (the stick overwrites it in the stage afterward).
pub fn decide(
    player: &PlayerSim,
    assignment: &ResolvedAssignment,
    role: &mut RoleState,
    commitment: &mut Option<Commitment>,
    ctx: &BrainCtx<'_>,
    user_controlled: bool,
) -> PlayerIntent {
    if !player.anim.can_act() {
        *role = RoleState::Down;
        *commitment = None;
        return PlayerIntent::Recover;
    }
    if *role == RoleState::Down {
        // Back on our feet: resume the assignment.
        *role = RoleState::Waiting;
    }

    let mut candidates: Vec<ScoredAction> = Vec::with_capacity(6);
    // A player who holds the ball in a LIVE play carries it, whatever their
    // assignment says (the catch promotes a receiver to carrier without new
    // data). After the whistle the holder just stands with the ball.
    if ctx.live && ctx.possession == Some(player.id) && !is_quarterback(assignment) {
        *role = RoleState::Carrying;
        offense::carry_candidates(player, ctx, &mut candidates);
    } else {
        match assignment.kind {
            AssignmentKind::Quarterback { .. }
            | AssignmentKind::Snapper
            | AssignmentKind::Route { .. }
            | AssignmentKind::PassBlock
            | AssignmentKind::LeadBlock
            | AssignmentKind::BallCarry => {
                offense::candidates(player, assignment, role, ctx, &mut candidates)
            }
            AssignmentKind::ManCover { .. }
            | AssignmentKind::ZoneCover { .. }
            | AssignmentKind::QuarterbackRush { .. }
            | AssignmentKind::EdgeContain { .. }
            | AssignmentKind::Pursuit
            | AssignmentKind::TackleTarget => {
                defense::candidates(player, assignment, role, ctx, &mut candidates)
            }
        }
    }

    if candidates.is_empty() {
        candidates.push(ScoredAction::new(
            PlayerIntent::Hold,
            Priority::Leverage,
            0.0,
            "idle",
            1,
        ));
    }
    commitment::arbitrate(&candidates, commitment, ctx.tick, user_controlled)
}

fn is_quarterback(assignment: &ResolvedAssignment) -> bool {
    matches!(assignment.kind, AssignmentKind::Quarterback { .. })
}
