//! The attempt: one explicit state machine, stepped at a fixed 60 Hz.
//!
//! The session owns the whole of an attempt and nothing outside it. It takes
//! [`PlayCommand`]s — never pointers, never pixels, never a drawing — and
//! produces a state the presentation layers read. Everything it does is a pure function of
//! `(commands, tick)`, so a replay of the same commands is the same attempt.
//!
//! The one rule the whole file is arranged around: between the strike and the
//! resolution, the *only* thing that can change the ball's fate is a capsule
//! contact. There is no code path here that edits the trajectory once it has been
//! committed.

use axiom_kernel::DeterministicRng;

use crate::figure::{KickDrive, KickPlan, Swing};
use crate::pitch::{ball_spot, GoalMouth, NetImpulse};
use crate::shot::{ResolvedShot, ShotIntent};
use crate::tuning::{Tuning, DT};

use super::ball::Ball;
use super::keeper::Keeper;
use super::nerve::KeeperNerve;
use super::phase::Phase;
use super::resolution::{ShotResult, Tally};

mod flight;
mod memory;

/// What the drawing layer asks the session to do.
///
/// Two words, and that is on purpose. Whatever the player drew, the *only* thing
/// that crosses this boundary is a finished [`ShotIntent`] — so the session
/// cannot be told to nudge a shot, and there is no path by which gesture code
/// could reach the ball.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum PlayCommand {
    /// Take this shot. The intent is read from the drawing; the session bounds
    /// it and commits.
    Kick(ShotIntent),
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
    /// The striking leg, and how far into the kick the body is. The swing is
    /// state because it is physics: the contact tick is whatever the integration
    /// produces, so nothing else in here is allowed to guess it.
    swing: Swing,
    kick_tick: u32,
    /// How fast the ball left the boot, metres per second — measured off the
    /// ball on the tick it was struck, not read back off what the shot was
    /// authored at. If the two ever disagree it is the ball that is telling the
    /// truth, and it is the ball the readout should be showing.
    struck: Option<f32>,
    result: Option<ShotResult>,
    tally: Tally,
    net: Option<NetImpulse>,
    /// Where the last few shots finished, so the keeper can shade toward them.
    seen: Vec<axiom::prelude::Vec3>,
    /// The shootout's luck, drawn from here and nowhere else.
    rng: DeterministicRng,
    /// Whether this session faces the average keeper rather than a rolled one.
    steady: bool,
}

/// The seed a session uses when none is named.
pub const DEFAULT_SEED: u64 = 0x0BE4_D17_5EED;

impl Session {
    /// A fresh session at the default seed.
    pub fn new(tuning: Tuning) -> Session {
        Session::seeded(tuning, DEFAULT_SEED)
    }

    /// A session facing the **average** keeper: no jitter, no guesses, always
    /// corrects.
    ///
    /// This is what the mechanic tests play against, so that "a bent shot beats a
    /// keeper that read it straight" is a claim about the mechanic rather than
    /// about a lucky roll. Nothing a player ever meets is steady.
    pub fn steady(tuning: Tuning) -> Session {
        Session {
            steady: true,
            keeper: Keeper::set(KeeperNerve::steady(&tuning.keeper)),
            ..Session::seeded(tuning, DEFAULT_SEED)
        }
    }

    /// A fresh session on an explicit seed. The same seed is the same shootout.
    pub fn seeded(tuning: Tuning, seed: u64) -> Session {
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
            keeper: Keeper::set(KeeperNerve::steady(&tuning.keeper)),
            kick: KickPlan::for_shot(origin, KickDrive::for_shot(&intent, &tuning), &tuning.kick),
            swing: Swing::cocked(&tuning.kick),
            kick_tick: 0,
            struck: None,
            result: None,
            tally: Tally::default(),
            net: None,
            seen: Vec::new(),
            rng: DeterministicRng::seeded(seed),
            steady: false,
            mouth,
            tuning,
        }
        .with_first_nerve()
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
    pub fn swing(&self) -> &Swing {
        &self.swing
    }
    /// Ticks since the run-up began — the kick's own clock, which keeps running
    /// through the flight so the follow-through never restarts.
    pub fn kick_tick(&self) -> u32 {
        self.kick_tick
    }
    pub fn result(&self) -> Option<ShotResult> {
        self.result
    }
    /// The speed the ball left at, metres per second, once it has.
    pub fn struck_speed(&self) -> Option<f32> {
        self.struck
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
    pub fn step(&mut self, commands: &[PlayCommand]) {
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

    /// Apply one command. A kick that arrives outside the aiming stage is
    /// ignored rather than queued — a drawing finished by a phase change beneath
    /// the player's finger must not fire into the next attempt.
    fn apply(&mut self, command: PlayCommand) {
        match command {
            PlayCommand::Kick(intent) if self.phase.accepts_drawing() => {
                // The session bounds what it was handed. The reading is the
                // player's instruction; staying inside the shapes a kicker can
                // actually strike is the game's business, and it happens here,
                // once, on the way in.
                self.intent = ShotIntent {
                    target: intent.target,
                    shape: intent.shape.bounded(
                        (self.tuning.bend.min_offset, self.tuning.bend.max_offset),
                        (self.tuning.loft.min_offset, self.tuning.loft.max_offset),
                    ),
                    pace: intent.pace,
                };
                self.rebuild();
                self.phase = Phase::ShotReady;
                self.phase_tick = 0;
            }
            PlayCommand::Restart => self.reset(),
            PlayCommand::Kick(_) => {}
        }
    }

    /// Re-resolve the authored shot from the current intent, and with it the
    /// body that is going to strike it. The drawing decides both.
    fn rebuild(&mut self) {
        self.shot = ResolvedShot::build(self.shot.origin, self.intent, &self.mouth, &self.tuning);
        let drive = KickDrive::for_shot(&self.intent, &self.tuning);
        self.kick = KickPlan::for_shot(self.shot.origin, drive, &self.tuning.kick);
        self.swing = Swing::cocked(&self.tuning.kick);
        self.kick_tick = 0;
    }

    /// Set up a fresh attempt, keeping only the tally and the keeper's memory.
    /// The shot itself starts blank: the next drawing is the next instruction.
    fn reset(&mut self) {
        let origin = ball_spot(self.tuning.flight.ball_radius);
        self.intent = ShotIntent::default();
        self.ball = Ball::placed(origin);
        let (across, up, weight) = self.shade();
        let nerve = self.next_nerve();
        self.keeper = Keeper::shaded(across, up, weight, nerve);
        self.result = None;
        self.struck = None;
        self.net = None;
        self.phase = Phase::Ready;
        self.phase_tick = 0;
        self.rebuild();
    }

    /// The phase machine: every transition, in one place.
    fn advance_phase(&mut self) {
        let t = self.tuning.transitions;
        match self.phase {
            Phase::Ready => self.after(t.ready, Phase::Aiming),
            // Aiming ends only when a drawing does, which is a command, not a
            // timer: the player takes as long as they like over the line.
            Phase::Aiming => {}
            Phase::ShotReady => self.after(t.commit, Phase::Kicking),
            Phase::Kicking => self.kicking(),
            Phase::BallInFlight => {
                self.advance_swing();
                self.in_flight();
            }
            // The leg is still following through while the ball is in the air,
            // and for a beat after it has finished.
            Phase::Resolution => {
                self.advance_swing();
                self.after(t.resolution, Phase::Reset);
            }
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
    use crate::shot::{BendCurve, GoalTarget};

    /// The shot a drawing would have been read as.
    fn shot(h: f32, v: f32, bend: f32, bend_at: f32, loft: f32, loft_at: f32) -> ShotIntent {
        ShotIntent::curved(GoalTarget::new(h, v), BendCurve::through(bend_at, bend, 0.14), BendCurve::through(loft_at, loft, 0.14), crate::stroke::Pace::STEADY)
    }

    /// Settle into the aiming stage, against the AVERAGE keeper: these tests are
    /// about the mechanic, not about the dice.
    fn armed() -> Session {
        let mut session = Session::steady(Tuning::DEFAULT);
        while session.phase() != Phase::Aiming {
            session.step(&[]);
        }
        session
    }

    /// Take one shot and run it to a result.
    fn take(intent: ShotIntent) -> Session {
        let mut session = armed();
        session.step(&[PlayCommand::Kick(intent)]);
        let mut spent = 0;
        while session.result().is_none() && spent < 900 {
            session.step(&[]);
            spent += 1;
        }
        assert!(spent < 900, "the attempt must resolve");
        session
    }

    #[test]
    fn the_attempt_is_draw_then_watch() {
        let mut session = Session::steady(Tuning::DEFAULT);
        assert_eq!(session.phase(), Phase::Ready);
        let mut settle = 0;
        while session.phase() == Phase::Ready {
            session.step(&[]);
            settle += 1;
        }
        assert!(settle <= 10, "the settle is a beat, not a wait: {settle} ticks");
        assert_eq!(session.phase(), Phase::Aiming);
        // Aiming lasts as long as the player wants; nothing times it out.
        (0..600).for_each(|_| session.step(&[]));
        assert_eq!(session.phase(), Phase::Aiming);
        // One command commits the whole shot.
        session.step(&[PlayCommand::Kick(shot(0.5, 0.6, 1.0, 0.6, 0.9, 0.5))]);
        assert_eq!(session.phase(), Phase::ShotReady);
        while session.phase() == Phase::ShotReady {
            session.step(&[]);
        }
        assert_eq!(session.phase(), Phase::Kicking);
    }

    #[test]
    fn the_session_bounds_whatever_the_drawing_asked_for() {
        let mut session = armed();
        // A reading far outside anything a kicker could strike.
        session.step(&[PlayCommand::Kick(shot(0.4, 0.5, 40.0, 0.5, 40.0, 0.5))]);
        let tuning = Tuning::DEFAULT;
        let (bend, loft) = session.intent().shape.reach();
        assert!(bend.abs() <= tuning.bend.max_offset + 1.0e-3);
        assert!(loft.abs() <= tuning.loft.max_offset + 1.0e-3);
        // And the path it produced is still legal end to end.
        let points = session.shot().trajectory.points();
        assert_eq!(points[0], session.shot().origin);
        assert_eq!(*points.last().expect("a path"), session.shot().world_target);
        assert!(points.iter().all(|p| p.y >= tuning.flight.ball_radius - 1.0e-4));
    }

    #[test]
    fn a_kick_that_arrives_outside_the_aiming_stage_is_ignored() {
        let mut session = armed();
        session.step(&[PlayCommand::Kick(shot(0.5, 0.6, 1.0, 0.5, 0.9, 0.5))]);
        let committed = *session.intent();
        // A second reading, arriving a tick late, must not re-author the shot.
        session.step(&[PlayCommand::Kick(shot(-0.9, 0.1, -2.0, 0.5, 0.0, 0.5))]);
        assert_eq!(*session.intent(), committed);
        assert_eq!(session.phase(), Phase::ShotReady);
    }

    #[test]
    fn the_ball_leaves_on_the_tick_the_swing_reaches_it() {
        let mut session = armed();
        session.step(&[PlayCommand::Kick(shot(0.0, 0.5, 0.0, 0.5, 0.6, 0.5))]);
        while session.phase() != Phase::Kicking {
            session.step(&[]);
        }
        let spot = session.ball().position;
        // Through the whole run-up and swing the ball sits on the spot: nothing
        // launches it but the leg arriving.
        let mut ticks = 0;
        while session.phase() == Phase::Kicking && ticks < 400 {
            assert_eq!(session.ball().position, spot, "the ball waits for the boot");
            assert_eq!(session.swing().struck_at(), None);
            session.step(&[]);
            ticks += 1;
        }
        assert_eq!(session.phase(), Phase::BallInFlight);
        assert!(session.swing().struck_at().is_some(), "it was struck");
        assert!(session.swing().impact_rate() < 0.0, "and struck at speed");
    }

    #[test]
    fn a_harder_drawing_puts_the_ball_in_the_air_sooner_and_off_a_faster_leg() {
        // The same shot, drawn slowly and drawn quickly. Nothing about the
        // TARGET differs — only the tempo — so any difference here is the body.
        let played = [0.0f32, 1.0].map(|speed| {
            let mut session = armed();
            let mut intent = shot(0.0, 0.6, 0.0, 0.5, 0.6, 0.5);
            intent.pace = crate::stroke::Pace { speed, easing: 0.0 };
            session.step(&[PlayCommand::Kick(intent)]);
            while session.phase() != Phase::BallInFlight {
                session.step(&[]);
            }
            (session.kick_tick(), session.swing().impact_rate().abs())
        });
        assert!(
            played[1].0 < played[0].0,
            "a hurried penalty is struck sooner: {} vs {}",
            played[1].0,
            played[0].0
        );
        assert!(
            played[1].1 > played[0].1 * 1.15,
            "and off a faster leg: {:.2} vs {:.2}",
            played[1].1,
            played[0].1
        );
    }

    #[test]
    fn a_clean_shot_into_an_empty_corner_is_a_goal() {
        let session = take(shot(-0.95, 0.92, 0.0, 0.5, 0.2, 0.5));
        assert_eq!(session.result(), Some(ShotResult::Goal));
        assert_eq!(session.tally(), Tally { attempts: 1, goals: 1 });
        assert!(session.net_impulse().is_some(), "the net answers");
    }

    #[test]
    fn a_shot_straight_at_the_keeper_is_saved_by_its_actual_reach() {
        let session = take(shot(0.0, 0.25, 0.0, 0.5, 0.15, 0.5));
        assert_eq!(session.result(), Some(ShotResult::Save));
        assert_eq!(session.tally(), Tally { attempts: 1, goals: 0 });
        // A save is a real contact: the ball has been knocked off the path.
        assert_eq!(session.ball().motion, BallMotion::Free);
    }

    #[test]
    fn the_same_endpoint_can_be_saved_or_scored_depending_on_the_shape() {
        // One point in the goal — low, left of centre, well inside the keeper's
        // range — reached two ways.
        let plain = take(shot(-0.60, 0.75, 0.0, 0.5, 0.9, 0.5));
        let sculpted = take(shot(-0.60, 0.75, 2.0, 0.28, 0.9, 0.5));
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
        let mut session = take(shot(-0.95, 0.92, 0.0, 0.5, 0.2, 0.5));
        let hold = session.tuning().transitions.resolution
            + session.tuning().transitions.reset
            + session.tuning().transitions.ready
            + 3;
        (0..hold).for_each(|_| session.step(&[]));
        assert_eq!(session.phase(), Phase::Aiming, "straight back to drawing");
        assert_eq!(session.result(), None);
        assert_eq!(session.ball().motion, BallMotion::Placed);
        assert_eq!(session.keeper().read(), None);
        assert_eq!(session.tally().attempts, 1, "the tally survives the reset");
        // A restart mid-draw does the same thing.
        session.step(&[PlayCommand::Restart]);
        assert_eq!(session.phase(), Phase::Ready);
    }

    #[test]
    fn the_same_command_always_produces_the_same_attempt() {
        let a = take(shot(0.4, 0.6, 1.5, 0.5, 1.2, 0.5));
        let b = take(shot(0.4, 0.6, 1.5, 0.5, 1.2, 0.5));
        assert_eq!(a.result(), b.result());
        assert_eq!(a.ball().position, b.ball().position);
        assert_eq!(a.keeper().read(), b.keeper().read());
        assert_eq!(a.tick(), b.tick());
    }
}
