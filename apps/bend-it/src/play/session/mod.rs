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
use super::dive_call::DiveCall;
use super::phase::Phase;
use super::resolution::{ShotResult, Tally};
use super::shootout::{Shootout, Side};

pub use start::DEFAULT_SEED;

mod flight;
mod memory;
mod start;
mod tournament;
mod view;

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
    /// Dive **now**, there. Only meaningful on the kicks the player is keeping,
    /// and only once — the whole of the decision is when it arrives.
    Dive(DiveCall),
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
    /// The keeper's own clock, seconds, running from the moment the taker steps
    /// up rather than from the strike.
    ///
    /// The AI keeper's clock starts at the strike, because its reaction time is
    /// measured from seeing the ball leave. The **player's** cannot: the whole of
    /// keeping is that you may commit before the ball moves, and a clock that
    /// only starts at the strike would quietly make every early dive begin at the
    /// same instant as a late one — which is to say, would delete the decision.
    keep_clock: f32,
    /// The shootout this attempt belongs to: the score, the order, the rules.
    shootout: Shootout,
    /// Whose kick this one is. On [`Side::Them`] the rival takes it and the
    /// player is the body in the goal.
    side: Side,
}


impl Session {
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
            // The dive the player called. Accepted from the moment the rival
            // starts moving right up to the instant the ball crosses the line —
            // early is a guess with a full dive behind it, late is knowledge with
            // no time left to use it, and choosing between those two IS keeping.
            PlayCommand::Dive(call) if self.keeping() & self.phase.accepts_dive() => {
                self.keeper.called(call, self.keep_clock, &self.tuning.keeper);
            }
            PlayCommand::Restart => self.reset(),
            PlayCommand::Kick(_) | PlayCommand::Dive(_) => {}
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
        self.side = self.shootout.turn();
        self.intent = ShotIntent::default();
        self.ball = Ball::placed(origin);
        // Keeping, the body in the goal is the player's: no reaction jitter, no
        // guess, no mid-flight correction. The player IS the reaction, and a
        // keeper that also had nerves would be taking decisions out of their
        // hands and then blaming them for the result.
        let nerve = match self.keeping() {
            true => KeeperNerve::steady(&self.tuning.keeper),
            false => self.next_nerve(),
        };
        let (across, up, weight) = self.shade();
        self.keeper = match self.keeping() {
            true => Keeper::set(nerve),
            false => Keeper::shaded(across, up, weight, nerve),
        };
        // The keeper is watching a run-up, not a flight: its clock starts now.
        self.keeper.waiting();
        self.result = None;
        self.struck = None;
        self.net = None;
        self.phase = Phase::Ready;
        self.phase_tick = 0;
        self.keep_clock = 0.0;
        self.rebuild();
    }



    /// The phase machine: every transition, in one place.
    fn advance_phase(&mut self) {
        let t = self.tuning.transitions;
        match self.phase {
            Phase::Ready => self.after(t.ready, Phase::Aiming),
            // Aiming ends only when a drawing does, which is a command, not a
            // timer: the player takes as long as they like over the line.
            //
            // Unless it is not their kick. The rival does not wait to be told —
            // it steps up, and the player's decision starts when the run-up does.
            Phase::Aiming => {
                (self.keeping() & self.outcome().is_none()).then(|| self.take_for_rival());
            }
            // The keeper's clock runs through the run-up as well as the flight,
            // so a dive called while the taker is still walking in has genuinely
            // been travelling by the time the ball leaves.
            Phase::ShotReady => {
                self.keep_step();
                self.after(t.commit, Phase::Kicking);
            }
            Phase::Kicking => {
                self.keep_step();
                self.kicking();
            }
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
                // A decided shootout stops here. There is nothing left to take,
                // and rolling straight into the next penalty would throw away the
                // only moment the game has that is worth sitting still for.
                (self.outcome().is_none()).then(|| {
                    self.after(t.reset, Phase::Ready);
                    self.phase_is(Phase::Ready).then(|| self.reset());
                });
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
    use axiom::prelude::Vec3;
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
        let plain = take(shot(-0.25, 0.70, 0.0, 0.5, 0.9, 0.5));
        let sculpted = take(shot(-0.25, 0.70, 2.0, 0.28, 0.9, 0.5));
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
    fn an_attempt_resets_cleanly_and_hands_over() {
        let mut session = take(shot(-0.95, 0.92, 0.0, 0.5, 0.2, 0.5));
        let hold = session.tuning().transitions.resolution
            + session.tuning().transitions.reset
            + session.tuning().transitions.ready
            + 3;
        (0..hold).for_each(|_| session.step(&[]));
        // The attempt is gone and the ball is back on the spot — but it is not
        // back to drawing, because it is not the player's kick any more. The
        // rival steps up on its own.
        assert_eq!(session.result(), None);
        assert_eq!(session.ball().motion, BallMotion::Placed);
        assert_eq!(session.side(), Side::Them);
        assert!(session.keeping(), "the player is in the goal now");
        assert_eq!(session.tally().attempts, 1, "the tally survives the reset");
        assert_eq!(session.shootout().score(), (1, 0));
        // A restart mid-attempt still sets up a fresh one.
        session.step(&[PlayCommand::Restart]);
        assert_eq!(session.phase(), Phase::Ready);
    }

    /// Run whatever attempt is up to a result, issuing whatever `on_tick` says.
    fn resolve(session: &mut Session, mut on_tick: impl FnMut(&Session) -> Vec<PlayCommand>) {
        let mut spent = 0;
        while session.result().is_none() && spent < 1200 {
            let commands = on_tick(session);
            session.step(&commands);
            spent += 1;
        }
        assert!(spent < 1200, "the attempt must resolve");
    }

    /// One call, issued the first tick `when` says it is due.
    fn call_once(
        call: DiveCall,
        when: impl Fn(&Session) -> bool,
    ) -> impl FnMut(&Session) -> Vec<PlayCommand> {
        let mut sent = false;
        move |s: &Session| {
            let due = when(s) & !sent;
            sent |= due;
            [Vec::new(), vec![PlayCommand::Dive(call)]][usize::from(due)].clone()
        }
    }

    /// Get to the point where the player is the one in the goal.
    fn keeping_now() -> Session {
        let mut session = Session::steady(Tuning::DEFAULT);
        while !session.phase().accepts_drawing() {
            session.step(&[]);
        }
        session.step(&[PlayCommand::Kick(shot(0.0, 0.25, 0.0, 0.5, 0.5, 0.5))]);
        resolve(&mut session, |_| Vec::new());
        let mut spent = 0;
        while !session.keeping() && spent < 600 {
            session.step(&[]);
            spent += 1;
        }
        assert!(session.keeping(), "it should be their kick by now");
        // And past the beat where the rival is still stepping up.
        while !session.phase().accepts_dive() {
            session.step(&[]);
        }
        session
    }

    #[test]
    fn a_whole_shootout_plays_itself_out_and_ends_decided() {
        let mut session = Session::steady(Tuning::DEFAULT);
        let dive = Vec3::new(-2.4, 1.0, 0.76);
        let mut guard = 0;
        while session.outcome().is_none() && guard < 60 {
            guard += 1;
            // Get to a moment where this attempt can be acted on.
            let mut settle = 0;
            while !session.phase().accepts_drawing()
                && !session.phase().accepts_dive()
                && session.outcome().is_none()
                && settle < 400
            {
                session.step(&[]);
                settle += 1;
            }
            (session.outcome().is_some()).then(|| ());
            match session.keeping() {
                true => resolve(
                    &mut session,
                    call_once(
                        DiveCall { hands: dive, lean: -0.6, height: 0.0 },
                        |s| s.phase().accepts_dive(),
                    ),
                ),
                false => {
                    session.step(&[PlayCommand::Kick(shot(-0.95, 0.35, 0.0, 0.5, 0.6, 0.5))]);
                    resolve(&mut session, |_| Vec::new());
                }
            }
        }
        let outcome = session.outcome().expect("a shootout ends");
        let (you, them) = session.shootout().score();
        assert_ne!(you, them, "{outcome:?} at {you}-{them} is not a result");
        assert!(session.shootout().taken_by(Side::You) > 0);
        assert!(session.shootout().taken_by(Side::Them) > 0);
        // Once decided, nothing else is taken however long it is left running.
        let settled = session.shootout().taken().len();
        (0..900).for_each(|_| session.step(&[]));
        assert_eq!(session.shootout().taken().len(), settled, "it kept playing");
        assert_eq!(session.outcome(), Some(outcome));
    }

    #[test]
    fn a_keeper_that_is_not_called_never_moves() {
        let mut standing = keeping_now();
        resolve(&mut standing, |_| Vec::new());
        assert_eq!(standing.keeper().read(), None, "it dived on its own");
        assert!(standing.keeper().motion().hips.x.abs() < 0.05, "it drifted");
    }

    #[test]
    fn a_called_dive_goes_exactly_where_it_was_called_and_nowhere_else() {
        let call = DiveCall {
            hands: Vec3::new(2.2, 1.1, 0.76),
            lean: 0.8,
            height: 0.2,
        };
        let mut dived = keeping_now();
        resolve(&mut dived, call_once(call, |s| s.phase().accepts_dive()));
        let read = dived.keeper().read().expect("it committed");
        // The HIPS stop an arm short of where the hands were called to — the same
        // rule the rival keeper lives under, so the player is not handed a longer
        // body than the one that beats them at the other end.
        let stretch = crate::figure::stretch_from_hips(&dived.tuning().keeper);
        assert!(
            ((call.hands.x - read.aim.x) - stretch).abs() < 0.15,
            "the hips went to {} for hands called at {}",
            read.aim.x,
            call.hands.x
        );
        assert!(dived.keeper().motion().hips.x > 0.3, "and it actually moved");
        // One dive. A second call after it has gone changes nothing.
        let before = dived.keeper().read();
        dived.step(&[PlayCommand::Dive(DiveCall {
            hands: Vec3::new(-3.0, 0.4, 0.76),
            lean: -1.0,
            height: -1.0,
        })]);
        assert_eq!(dived.keeper().read(), before, "it re-decided");
    }

    #[test]
    fn diving_early_reaches_further_than_diving_late() {
        // The whole of keeping, as one assertion. The same call, released at the
        // top of the run-up and released once the ball is already in the air.
        let session = keeping_now();
        let call = DiveCall {
            hands: Vec3::new(3.30, 1.0, 0.76),
            lean: 1.0,
            height: 0.0,
        };
        let reach = |wait_for_flight: bool| {
            let mut s = session.clone();
            resolve(
                &mut s,
                call_once(call, move |s: &Session| match wait_for_flight {
                    true => s.phase() == Phase::BallInFlight,
                    false => s.phase().accepts_dive(),
                }),
            );
            s.keeper().motion().hips.x
        };
        let (early, late) = (reach(false), reach(true));
        assert!(
            early > late + 0.4,
            "early got {early:.2} m and late got {late:.2} m — the bet is not a bet"
        );
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
