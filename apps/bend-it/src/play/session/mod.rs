//! The attempt: one explicit state machine, stepped at a fixed 60 Hz.
//!
//! The session owns the whole of an attempt and nothing outside it. It takes
//! [`EditorCommand`]s — never pointers, never pixels — and produces a state the
//! presentation layers read. Everything it does is a pure function of
//! `(commands, tick)`, so a replay of the same commands is the same attempt.
//!
//! The one rule the whole file is arranged around: between the strike and the
//! resolution, the *only* thing that can change the ball's fate is a capsule
//! contact. There is no code path here that edits the trajectory once it has been
//! committed.

use crate::figure::KickPlan;
use crate::pitch::{ball_spot, GoalMouth, NetImpulse};
use crate::shot::{BendCurve, GoalTarget, ResolvedShot, ShotIntent};
use crate::tuning::{Tuning, DT};

use super::ball::Ball;
use super::keeper::Keeper;
use super::phase::Phase;
use super::resolution::{ShotResult, Tally};

mod flight;

/// What the editor asks the session to do. The editor may say only these five
/// things, which is why gesture code can never reach into the shot.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum EditorCommand {
    /// Put the finish here.
    Aim(GoalTarget),
    /// Replace the top-down projection.
    SetBend(BendCurve),
    /// Replace the side projection.
    SetLoft(BendCurve),
    /// Go on to the next stage (and, from the last one, take the kick).
    Advance,
    /// Go back to the previous stage.
    Back,
    /// Abandon this attempt and set up another.
    Restart,
}

/// One attempt, plus the tally across the session.
#[derive(Debug, Clone)]
pub struct Session {
    tuning: Tuning,
    mouth: GoalMouth,
    phase: Phase,
    phase_tick: u32,
    tick: u64,
    intent: ShotIntent,
    shot: ResolvedShot,
    ball: Ball,
    keeper: Keeper,
    kick: KickPlan,
    result: Option<ShotResult>,
    tally: Tally,
    net: Option<NetImpulse>,
    /// Where the last few shots finished, so the keeper can shade toward them.
    seen: Vec<axiom::prelude::Vec3>,
}

/// How many past shots the keeper's shading averages over.
pub(super) const SHADE_MEMORY: usize = 4;

impl Session {
    /// A fresh session, set up for its first attempt.
    pub fn new(tuning: Tuning) -> Session {
        let mouth = GoalMouth::new(tuning.goal.inset);
        let origin = ball_spot(tuning.flight.ball_radius);
        let intent = ShotIntent::default();
        let shot = ResolvedShot::build(origin, intent, &mouth, &tuning);
        Session {
            phase: Phase::Ready,
            phase_tick: 0,
            tick: 0,
            intent,
            shot,
            ball: Ball::placed(origin),
            keeper: Keeper::set(),
            kick: KickPlan::for_shot(origin, 0.0, &tuning.kick),
            result: None,
            tally: Tally::default(),
            net: None,
            seen: Vec::new(),
            mouth,
            tuning,
        }
    }

    /// Where the keeper stands for the next penalty, and how high it expects the
    /// ball: shaded toward the average of the last few finishes, bounded so it
    /// never abandons the middle of the goal.
    fn shade(&self) -> (f32, f32, f32) {
        let count = self.seen.len().max(1) as f32;
        let gain = self.tuning.keeper.shade_gain;
        let across = self.seen.iter().map(|p| p.x).sum::<f32>() / count;
        let up = self.seen.iter().map(|p| p.y).sum::<f32>() / count;
        let weight = gain * (self.seen.len() as f32 / SHADE_MEMORY as f32).min(1.0);
        (
            (across * gain).clamp(
                -self.tuning.keeper.shade_limit,
                self.tuning.keeper.shade_limit,
            ),
            [1.0, up][usize::from(!self.seen.is_empty())],
            weight,
        )
    }

    pub fn phase(&self) -> Phase {
        self.phase
    }
    pub fn phase_tick(&self) -> u32 {
        self.phase_tick
    }
    pub fn tick(&self) -> u64 {
        self.tick
    }
    pub fn intent(&self) -> &ShotIntent {
        &self.intent
    }
    pub fn shot(&self) -> &ResolvedShot {
        &self.shot
    }
    pub fn ball(&self) -> &Ball {
        &self.ball
    }
    pub fn keeper(&self) -> &Keeper {
        &self.keeper
    }
    pub fn kick(&self) -> &KickPlan {
        &self.kick
    }
    pub fn result(&self) -> Option<ShotResult> {
        self.result
    }
    pub fn tally(&self) -> Tally {
        self.tally
    }
    pub fn tuning(&self) -> &Tuning {
        &self.tuning
    }
    pub fn mouth(&self) -> &GoalMouth {
        &self.mouth
    }
    pub fn net_impulse(&self) -> Option<NetImpulse> {
        self.net
    }

    /// Advance one fixed tick.
    pub fn step(&mut self, commands: &[EditorCommand]) {
        commands.iter().for_each(|c| self.apply(*c));
        let before = self.phase;
        self.advance_phase();
        self.phase_tick = match self.phase == before {
            true => self.phase_tick + 1,
            false => 0,
        };
        self.tick += 1;
        self.net = self.net.map(|n| NetImpulse {
            age: n.age + DT,
            ..n
        });
    }

    /// Apply one editor command. Commands that do not belong to the current
    /// phase are ignored rather than queued — a stale tap from before a
    /// transition must not fire into the next stage.
    fn apply(&mut self, command: EditorCommand) {
        match command {
            EditorCommand::Aim(target) if self.phase.accepts_aim() => {
                self.intent.target = target;
                self.rebuild();
            }
            EditorCommand::SetBend(curve) if self.phase == Phase::HorizontalSculpt => {
                self.intent.bend = curve.bounded(
                    self.tuning.bend.min_offset,
                    self.tuning.bend.max_offset,
                );
                self.rebuild();
            }
            EditorCommand::SetLoft(curve) if self.phase == Phase::VerticalSculpt => {
                self.intent.loft = curve
                    .bounded(self.tuning.loft.min_offset, self.tuning.loft.max_offset);
                self.rebuild();
            }
            EditorCommand::Advance => {
                self.phase = self.phase.advanced().unwrap_or(self.phase);
                self.phase_tick = 0;
            }
            EditorCommand::Back => {
                self.phase = self.phase.backed().unwrap_or(self.phase);
                self.phase_tick = 0;
            }
            EditorCommand::Restart => self.reset(),
            _ => {}
        }
    }

    /// Re-resolve the authored shot. Cheap enough to do on every edit, which is
    /// what makes the 3D preview track the finger instead of lagging it.
    fn rebuild(&mut self) {
        self.shot = ResolvedShot::build(self.shot.origin, self.intent, &self.mouth, &self.tuning);
        let (bend, _) = self.intent.effort(&self.tuning);
        let signed = bend * self.intent.bend.magnitude().signum();
        self.kick = KickPlan::for_shot(self.shot.origin, signed, &self.tuning.kick);
    }

    /// Set up a fresh attempt, keeping the tally and the last aim (a player
    /// taking ten penalties should not have to re-find the same corner).
    fn reset(&mut self) {
        let origin = ball_spot(self.tuning.flight.ball_radius);
        self.intent = ShotIntent::opening(self.intent.target);
        self.ball = Ball::placed(origin);
        let (across, up, weight) = self.shade();
        self.keeper = Keeper::shaded(across, up, weight);
        self.result = None;
        self.net = None;
        self.phase = Phase::Ready;
        self.phase_tick = 0;
        self.rebuild();
    }

    /// The phase machine: every transition, in one place.
    fn advance_phase(&mut self) {
        let t = &self.tuning.transitions;
        match self.phase {
            Phase::Ready => self.after(t.ready, Phase::TargetSelection),
            Phase::TargetSelection | Phase::HorizontalSculpt | Phase::VerticalSculpt => {}
            Phase::ShotReady => self.after(t.commit, Phase::Kicking),
            Phase::Kicking => self.kicking(),
            Phase::BallInFlight => self.in_flight(),
            Phase::Resolution => self.after(t.resolution, Phase::Reset),
            Phase::Reset => {
                self.after(t.reset, Phase::Ready);
                // Entering Ready through a reset rebuilds the attempt.
                self.phase_is(Phase::Ready).then(|| self.reset());
            }
        }
    }

    fn phase_is(&self, phase: Phase) -> bool {
        self.phase == phase
    }

    fn after(&mut self, ticks: u32, next: Phase) {
        self.phase = match self.phase_tick + 1 >= ticks {
            true => next,
            false => self.phase,
        };
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::play::ball::BallMotion;
    use crate::play::phase::Phase;

    /// Drive a session to a result, feeding `commands` on the first tick.
    fn play(commands: &[EditorCommand]) -> Session {
        let mut session = Session::new(Tuning::DEFAULT);
        // Get out of Ready and into the editor.
        (0..12).for_each(|_| session.step(&[]));
        session.step(commands);
        let mut spent = 0;
        while session.result().is_none() && spent < 900 {
            session.step(&[]);
            spent += 1;
        }
        assert!(spent < 900, "the attempt must resolve");
        session
    }

    fn shape(h: f32, v: f32, bend: f32, loft: f32) -> Vec<EditorCommand> {
        shaped(h, v, bend, 0.5, loft, 0.5)
    }

    /// The same, with explicit peak positions — so a test can say "this one
    /// breaks late" rather than only "this one breaks".
    fn shaped(
        h: f32,
        v: f32,
        bend: f32,
        bend_at: f32,
        loft: f32,
        loft_at: f32,
    ) -> Vec<EditorCommand> {
        vec![
            EditorCommand::Aim(GoalTarget::new(h, v)),
            EditorCommand::Advance,
            EditorCommand::SetBend(BendCurve::through(bend_at, bend, 0.14)),
            EditorCommand::Advance,
            EditorCommand::SetLoft(BendCurve::through(loft_at, loft, 0.14)),
            EditorCommand::Advance,
        ]
    }

    #[test]
    fn the_flow_walks_forward_through_every_stage() {
        let mut session = Session::new(Tuning::DEFAULT);
        assert_eq!(session.phase(), Phase::Ready);
        (0..12).for_each(|_| session.step(&[]));
        assert_eq!(session.phase(), Phase::TargetSelection);
        session.step(&[EditorCommand::Advance]);
        assert_eq!(session.phase(), Phase::HorizontalSculpt);
        session.step(&[EditorCommand::Advance]);
        assert_eq!(session.phase(), Phase::VerticalSculpt);
        session.step(&[EditorCommand::Advance]);
        assert_eq!(session.phase(), Phase::ShotReady);
        (0..20).for_each(|_| session.step(&[]));
        assert_eq!(session.phase(), Phase::Kicking);
    }

    #[test]
    fn the_player_can_go_back_and_change_their_mind() {
        let mut session = Session::new(Tuning::DEFAULT);
        (0..12).for_each(|_| session.step(&[]));
        session.step(&[EditorCommand::Advance, EditorCommand::Advance]);
        assert_eq!(session.phase(), Phase::VerticalSculpt);
        session.step(&[EditorCommand::Back]);
        assert_eq!(session.phase(), Phase::HorizontalSculpt);
        // Re-aiming works from a sculpt stage, without losing the curve.
        session.step(&[EditorCommand::SetBend(BendCurve::through(0.5, 3.0, 0.14))]);
        let bend = session.intent().bend;
        session.step(&[EditorCommand::Aim(GoalTarget::new(-0.9, 0.8))]);
        assert_eq!(session.intent().bend, bend);
        assert!((session.intent().target.h + 0.9).abs() < 1.0e-5);
        // A stale command for the wrong stage is ignored, not queued.
        session.step(&[EditorCommand::SetLoft(BendCurve::through(0.5, 3.0, 0.14))]);
        assert_eq!(session.intent().loft, ShotIntent::default().loft);
    }

    #[test]
    fn the_ball_launches_on_the_contact_tick_and_not_before() {
        let mut session = Session::new(Tuning::DEFAULT);
        (0..12).for_each(|_| session.step(&[]));
        session.step(&shape(0.0, 0.5, 0.0, 0.6));
        while session.phase() != Phase::Kicking {
            session.step(&[]);
        }
        // Count from the first tick of the run-up: the ball must not move until
        // the tick the boot reaches it, and must move on exactly that tick.
        let spot = session.ball().position;
        let contact = session.tuning().kick.contact;
        (session.phase_tick()..contact - 1).for_each(|_| {
            session.step(&[]);
            assert_eq!(session.phase(), Phase::Kicking);
            assert_eq!(session.ball().position, spot, "the ball waits for the boot");
        });
        session.step(&[]);
        assert_eq!(session.phase(), Phase::BallInFlight);
    }

    #[test]
    fn a_clean_shot_into_an_empty_corner_is_a_goal() {
        // A hard, flat shot into the top corner, well past the keeper's reach.
        let session = play(&shape(-0.95, 0.92, 0.0, 0.2));
        assert_eq!(session.result(), Some(ShotResult::Goal));
        assert_eq!(session.tally(), Tally { attempts: 1, goals: 1 });
        assert!(session.net_impulse().is_some(), "the net answers");
    }

    #[test]
    fn a_shot_straight_at_the_keeper_is_saved_by_its_actual_reach() {
        let session = play(&shape(0.0, 0.25, 0.0, 0.15));
        assert_eq!(session.result(), Some(ShotResult::Save));
        assert_eq!(session.tally(), Tally { attempts: 1, goals: 0 });
        // A save is a real contact: the ball has been knocked off the path.
        assert_eq!(session.ball().motion, BallMotion::Free);
    }

    #[test]
    fn the_same_endpoint_can_be_saved_or_scored_depending_on_the_shape() {
        // One point in the goal — low, left of centre, well inside the keeper's
        // range — reached two ways.
        let plain = play(&shaped(-0.45, 0.30, 0.0, 0.5, 0.9, 0.5));
        let sculpted = play(&shaped(-0.45, 0.30, 2.0, 0.28, 0.9, 0.5));
        assert_eq!(
            plain.shot().world_target,
            sculpted.shot().world_target,
            "both shots finish at the same point"
        );
        assert_eq!(
            plain.result(),
            Some(ShotResult::Save),
            "driven straight at it, the keeper reads it and gets there"
        );
        assert_eq!(
            sculpted.result(),
            Some(ShotResult::Goal),
            "bent away early and swung back, the same point is open"
        );
    }

    #[test]
    fn an_attempt_resets_cleanly_and_quickly() {
        let mut session = play(&shape(-0.95, 0.92, 0.0, 0.2));
        let hold = session.tuning().transitions.resolution + session.tuning().transitions.reset + 2;
        (0..hold).for_each(|_| session.step(&[]));
        assert_eq!(session.phase(), Phase::Ready);
        assert_eq!(session.result(), None);
        assert_eq!(session.ball().motion, BallMotion::Placed);
        assert_eq!(session.keeper().read(), None);
        assert_eq!(session.tally().attempts, 1, "the tally survives the reset");
        // The aim survives too, but the sculpt is back to a plain shot.
        assert_eq!(session.intent().bend, BendCurve::STRAIGHT);
        // And a restart mid-edit does the same thing.
        (0..12).for_each(|_| session.step(&[]));
        session.step(&[EditorCommand::Restart]);
        assert_eq!(session.phase(), Phase::Ready);
    }

    #[test]
    fn the_same_commands_always_produce_the_same_attempt() {
        let a = play(&shape(0.4, 0.6, 2.5, 1.5));
        let b = play(&shape(0.4, 0.6, 2.5, 1.5));
        assert_eq!(a.result(), b.result());
        assert_eq!(a.ball().position, b.ball().position);
        assert_eq!(a.keeper().read(), b.keeper().read());
        assert_eq!(a.tick(), b.tick());
    }
}
