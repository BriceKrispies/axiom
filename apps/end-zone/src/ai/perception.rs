//! The shared, read-only play model every brain consumes so the whole team
//! reacts to the *same* football play. It carries only **delay-invariant derived
//! play facts** — the situation, the ball/catch geometry, the pocket, the
//! quarterback's run commitment, and the coordinated pursuit responsibilities.
//! It deliberately does **not** carry per-player delayed-opponent geometry:
//! defenders still sample the delayed [`super::Perception`] ring at their own
//! reaction delay for the opponent they chase, so individual reaction latency is
//! preserved while the shared *situation + responsibilities* make the team
//! coherent.

use axiom::prelude::Vec3;

use crate::config::PLAYER_COUNT;
use crate::data::BehaviorTuning;
use crate::field::{OffenseFrame, GOAL_LINE_Z};
use crate::football::{situation, BallSituation, BallState};
use crate::identity::{PlayerId, TeamId};
use crate::state::SimState;

use super::brain::RoleState;
use super::coordination;
use super::engagement::Engagement;

/// The protected pocket: the region behind the line of scrimmage the blockers
/// keep the rush out of, and the box the quarterback is "in" until he commits to
/// a scramble.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PocketRegion {
    pub center: Vec3,
    pub half_width: f32,
    /// How far behind the line of scrimmage the pocket extends, yards.
    pub depth: f32,
    /// How far past the line of scrimmage still counts as "in the pocket", yards.
    pub lip: f32,
    pub los_z: f32,
    pub sign: f32,
}

impl PocketRegion {
    /// The pocket for a drive frame.
    pub fn for_frame(frame: &OffenseFrame, tuning: &BehaviorTuning) -> Self {
        let sign = frame.direction.sign();
        let los = frame.line_of_scrimmage_z;
        PocketRegion {
            center: Vec3::new(0.0, 0.0, los - sign * tuning.pocket_depth * 0.5),
            half_width: tuning.pocket_half_width,
            depth: tuning.pocket_depth,
            lip: tuning.pocket_lip,
            los_z: los,
            sign,
        }
    }

    /// Whether a ground position sits inside the pocket box.
    pub fn contains(&self, pos: Vec3) -> bool {
        let downfield = (pos.z - self.los_z) * self.sign;
        pos.x.abs() < self.half_width && downfield <= self.lip && downfield >= -self.depth
    }
}

/// The coordinated pursuit responsibility a defender has been handed this tick
/// (spec §4, §8). Exactly one defender is the primary; the rest fill the
/// supporting lanes so nobody duplicates the same angle.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Responsibility {
    #[default]
    None,
    /// Attack the ball carrier directly.
    PrimaryTackler,
    /// Hold the outside escape lane.
    OutsideContain,
    /// Guard the cutback / inside lane.
    Cutback,
    /// Preserve deep, touchdown-preventing leverage.
    DeepHelp,
    /// Attack the ball at an interception point.
    Intercept,
    /// Contest the catch point.
    ContestCatch,
    /// Take a post-catch tackle angle on the receiver.
    TackleAngle,
}

impl Responsibility {
    pub fn label(self) -> &'static str {
        match self {
            Responsibility::None => "none",
            Responsibility::PrimaryTackler => "primary",
            Responsibility::OutsideContain => "contain",
            Responsibility::Cutback => "cutback",
            Responsibility::DeepHelp => "deep",
            Responsibility::Intercept => "intercept",
            Responsibility::ContestCatch => "contest",
            Responsibility::TackleAngle => "tackle-angle",
        }
    }
}

/// The shared play model for one tick.
#[derive(Debug, Clone)]
pub struct PlayPerception {
    pub tick: u64,
    pub situation: BallSituation,
    pub carrier: Option<PlayerId>,
    /// The live ground ball carrier the defense should rally to (a scrambling
    /// quarterback or a receiver running after the catch).
    pub ground_threat: Option<PlayerId>,
    pub ball_pos: Vec3,
    pub ball_vel: Vec3,
    /// The predicted catch point of a live pass, if one is in the air.
    pub catch_point: Option<Vec3>,
    /// The tick the live pass is predicted to arrive.
    pub eta_tick: Option<u64>,
    /// The intended receiver of a live pass.
    pub intended_receiver: Option<PlayerId>,
    pub quarterback: PlayerId,
    pub qb_pos: Vec3,
    pub qb_vel: Vec3,
    pub qb_in_pocket: bool,
    pub qb_committed_to_run: bool,
    pub pocket: PocketRegion,
    /// The centre of the end zone the offense is attacking.
    pub end_zone: Vec3,
    pub drive_sign: f32,
    pub offense_team: TeamId,
    /// Per-player coordinated responsibility, indexed by [`PlayerId`].
    pub responsibilities: [Responsibility; PLAYER_COUNT],
}

impl PlayPerception {
    /// This player's coordinated responsibility.
    pub fn responsibility(&self, player: PlayerId) -> Responsibility {
        self.responsibilities[player.index()]
    }

    /// Whether `player` is on the offense.
    pub fn is_offense(&self, team: TeamId) -> bool {
        team == self.offense_team
    }
}

impl SimState {
    /// Advance the derived ball situation for this tick (the ONLY writer of the
    /// scramble counter) and return it. Called first in the AI stage.
    pub(crate) fn update_ai_situation(&mut self) -> BallSituation {
        let pocket = PocketRegion::for_frame(&self.frame, &self.tuning);
        let qb = self.players[self.quarterback.index()];
        let holds = self.possession == Some(self.quarterback);
        let in_pocket = pocket.contains(qb.pos);
        let downfield_speed = qb.vel.dot(self.frame.forward());
        let showing_run = holds && !in_pocket && downfield_speed > self.tuning.scramble_speed;
        self.ai_memory.qb_downfield_ticks = if showing_run {
            (self.ai_memory.qb_downfield_ticks + 1).min(self.tuning.scramble_commit_ticks + 4)
        } else {
            0
        };
        let qb_run = holds && self.ai_memory.qb_downfield_ticks >= self.tuning.scramble_commit_ticks;
        let qb_windup = matches!(
            self.roles[self.quarterback.index()],
            RoleState::QbWindup { .. }
        );
        let contested = self.pass_is_contested();
        let sit = situation::classify(
            &self.ball,
            self.quarterback,
            self.phase,
            qb_windup,
            qb_run,
            contested,
        );
        self.ai_memory.situation = sit;
        sit
    }

    /// Whether a live pass has a defender inside the catch window.
    fn pass_is_contested(&self) -> bool {
        let BallState::Airborne { flight } = self.ball.state else {
            return false;
        };
        let recv_team = self.players[flight.intended.index()].team;
        self.players.iter().any(|p| {
            p.team != recv_team
                && p.anim.can_act()
                && flat(p.pos.subtract(flight.target)).length() < self.tuning.contest_radius
        })
    }

    /// Build the shared, read-only play perception for this tick.
    pub(crate) fn build_play_perception(&self, situation: BallSituation) -> PlayPerception {
        let pocket = PocketRegion::for_frame(&self.frame, &self.tuning);
        let qb = self.players[self.quarterback.index()];
        let carrier = self.ball.carrier();
        let ground_threat = match situation {
            BallSituation::QbScramble => Some(self.quarterback),
            BallSituation::Caught => carrier,
            _ => None,
        };
        let (catch_point, eta_tick, intended_receiver) = match self.ball.state {
            BallState::Airborne { flight } => (
                Some(flight.target),
                Some(flight.arrival_tick()),
                Some(flight.intended),
            ),
            _ => (None, None, None),
        };
        // The end zone centre: the attacked goal line plus five yards, on the
        // drive side, on the centre line.
        let end_zone = Vec3::new(0.0, 0.0, self.frame.direction.sign() * (GOAL_LINE_Z + 5.0));
        let mut per = PlayPerception {
            tick: self.tick,
            situation,
            carrier,
            ground_threat,
            ball_pos: self.ball.pos,
            ball_vel: self.ball.vel,
            catch_point,
            eta_tick,
            intended_receiver,
            quarterback: self.quarterback,
            qb_pos: qb.pos,
            qb_vel: qb.vel,
            qb_in_pocket: pocket.contains(qb.pos),
            qb_committed_to_run: situation == BallSituation::QbScramble,
            pocket,
            end_zone,
            drive_sign: self.frame.direction.sign(),
            offense_team: self.play.possession,
            responsibilities: [Responsibility::None; PLAYER_COUNT],
        };
        coordination::assign_responsibilities(&mut per, &self.players, &self.tuning);
        per
    }

    /// The football situation the AI derived this tick (debug overlay + tests).
    pub fn ball_situation(&self) -> BallSituation {
        self.ai_memory.situation
    }

    /// A defender's coordinated pursuit responsibility this tick.
    pub fn responsibility(&self, player: PlayerId) -> Responsibility {
        self.ai_memory.responsibilities[player.index()]
    }

    /// A player's committed-action debug reason, if committed.
    pub fn commitment_reason(&self, player: PlayerId) -> Option<&'static str> {
        self.ai_memory.commitments[player.index()].map(|c| c.reason)
    }

    /// Ticks of committed action `player` has left before it may freely switch.
    pub fn commitment_ticks_left(&self, player: PlayerId) -> u32 {
        self.ai_memory
            .commitment_ticks_left(player.index(), self.tick)
    }

    /// A blocker's current line engagement, if he is engaged.
    pub fn engagement(&self, blocker: PlayerId) -> Option<Engagement> {
        self.engagements[blocker.index()]
    }
}

fn flat(v: Vec3) -> Vec3 {
    Vec3::new(v.x, 0.0, v.z)
}
