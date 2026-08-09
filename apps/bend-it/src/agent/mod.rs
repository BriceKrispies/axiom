//! An embodied agent that plays Bend It, through the reusable `axiom-agent`
//! substrate.
//!
//! The game is playable by a machine for the same reason it is playable by a
//! thumb: there is one gesture, and its whole output is **pixels**. So the agent
//! gets no special interface, no cheat hook and no scripted intent — it **draws a
//! line**, the same line a finger would leave, and the kicker reads it with
//! exactly the same code and exactly the same loss.
//!
//! ```text
//! session state --perceive--> Observation (integer facts)   the striker's eyes
//!               --axiom-agent decide--> move_axis intents   the striker's hands
//!               --render--> a Stroke, in screen pixels      the striker's finger
//!               --the game's own reader--> ShotIntent --> session.step()
//! ```
//!
//! # What is the agent's, and what is the app's
//!
//! **The app owns perception.** Which corner is open, whether the keeper went the
//! right way last time, how late a curve has to break — every part of that names
//! a Bend It noun that `axiom-agent` must never learn.
//!
//! **The agent owns the control law.** A table of neutral bindings, each turning
//! a perceived scalar into a deflection of a control axis with a gain and limits.
//! It contains no soccer at all; the same table shape would fly a plane. Nothing
//! here hand-rolls the decision: every axis of every shot is emitted by
//! `AgentApi::step` as a `move_axis` intent, and the app's only remaining job is
//! to turn those axes into a line on the screen.
//!
//! # Why it draws instead of authoring
//!
//! Handing the session a finished `ShotIntent` would be easier and would prove
//! nothing. Drawing means the agent goes through the reading — the fit, the
//! clamp, the resampling — so a play-through measures whether the drawing channel
//! is actually good enough to play the game with. If interpretation ever got
//! sloppy, the agent's score would fall first.

use axiom::prelude::Vec2;
use axiom_agent::AgentApi;
use axiom_kernel::{FrameIndex, Tick};
use axiom_runtime::RuntimeStep;

use crate::camera;
use crate::pitch::GoalMouth;
use crate::play::{Phase, PlayCommand, Session, ShotResult};

pub use eyes::nonzero_sign;
use crate::projection::ScreenProjection;
use crate::stroke::interpret;

/// The app's control-axis vocabulary: the meaning this app assigns to a neutral
/// `move_axis` code. `axiom-agent` carries the `u32` opaquely.
pub const AXIS_AIM_H: u32 = 1;
pub const AXIS_AIM_V: u32 = 2;
pub const AXIS_BEND: u32 = 3;
pub const AXIS_BREAK_AT: u32 = 4;
pub const AXIS_LOFT: u32 = 5;
pub const AXIS_PACE: u32 = 6;

/// The app's observation-fact vocabulary: what the striker can *see*. Values are
/// milli-units, because agent facts are integer only.
pub const FACT_OPEN_SIDE: u16 = 10;
pub const FACT_OPEN_HEIGHT: u16 = 11;
pub const FACT_BEND_DEMAND: u16 = 12;
pub const FACT_BREAK_LATENESS: u16 = 13;
pub const FACT_LOFT_DEMAND: u16 = 14;
pub const FACT_PACE_DEMAND: u16 = 15;

/// One milli-unit.
const MILLI: f32 = 1000.0;
/// How many points the striker's finger leaves on the glass.
pub(super) const HAND_SAMPLES: usize = 26;
/// How unsteady that finger is, in pixels — deterministic, but enough that the
/// agent is drawing rather than tracing a perfect curve.
pub(super) const HAND_TREMOR: f32 = 3.2;

/// The phone screen the agent plays on. It draws on a viewport like anyone else.
pub const AGENT_VIEWPORT: Vec2 = Vec2::new(390.0, 844.0);

/// What the striker remembers about the last attempt.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Recall {
    pub aimed: f32,
    pub keeper_went: f32,
    pub scored: bool,
}

/// One decision's worth of control-axis deflections.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(super) struct Axes {
    pub aim_h: f32,
    pub aim_v: f32,
    pub bend: f32,
    pub break_at: f32,
    pub loft: f32,
    pub pace: f32,
}

mod eyes;
mod hand;

/// The striker.
///
/// It holds only *app* state — what it saw, and how many penalties it has taken.
/// The agent itself is built fresh inside each decision, because every contract
/// type in `axiom-agent` is sealed behind its one facade and cannot be named in a
/// struct field. That is the Module Law working as intended: the binding table is
/// a pure value, the brain is stateless, and the only thing that genuinely
/// persists is a soccer memory, which is the app's to keep.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Striker {
    seed: u64,
    recall: Option<Recall>,
    attempt: u32,
    steps: u64,
    recorded: bool,
}

impl Striker {
    /// A striker. `seed` varies which corner it opens with.
    pub fn new(seed: u64) -> Striker {
        Striker {
            seed,
            recall: None,
            attempt: 0,
            steps: 0,
            recorded: false,
        }
    }

    pub fn recall(&self) -> Option<Recall> {
        self.recall
    }

    pub fn attempts(&self) -> u32 {
        self.attempt
    }

    /// Play one tick: watch, and — when the pitch is waiting for a line — draw
    /// one and let the game read it.
    pub fn play(&mut self, session: &mut Session, projection: &ScreenProjection) {
        self.remember(session);
        let commands = (session.phase() == Phase::Aiming)
            .then(|| self.stroke_for(session, projection))
            .flatten()
            .and_then(|line| {
                interpret(
                    &line,
                    projection,
                    session.shot().origin,
                    session.mouth(),
                    session.tuning(),
                )
            })
            .map(|reading| vec![PlayCommand::Kick(reading.intent)])
            .unwrap_or_default();
        session.step(&commands);
        self.steps += 1;
    }

    /// Fold a finished attempt into the recall, once.
    fn remember(&mut self, session: &Session) {
        let finished = session
            .result()
            .filter(|_| !self.recorded)
            .zip(session.keeper().read());
        if let Some((result, read)) = finished {
            self.recorded = true;
            self.attempt += 1;
            self.recall = Some(Recall {
                aimed: session.intent().target.h,
                keeper_went: read.aim.x,
                scored: result == ShotResult::Goal,
            });
        }
        self.recorded &= session.result().is_some();
    }

    /// Perceive, and let the agent's control law decide the shape.
    pub(super) fn decide(&mut self, session: &Session) -> Axes {
        let agent = AgentApi::create_agent_id(self.seed);
        let profile = AgentApi::debug_perfect_profile();
        // The control law. Every number here is the striker's *hands*: how hard
        // it commits to a side, how near the post it dares aim, how late it is
        // willing to break a curve. None of it names a soccer concept.
        let mut brain = AgentApi::axis_map_brain(vec![
            AgentApi::axis_binding(FACT_OPEN_SIDE, AXIS_AIM_H, 940, 0, -880, 880),
            AgentApi::axis_binding(FACT_OPEN_HEIGHT, AXIS_AIM_V, 1_000, 0, 80, 940),
            AgentApi::axis_binding(FACT_BEND_DEMAND, AXIS_BEND, 1_000, 0, -1_000, 1_000),
            AgentApi::axis_binding(FACT_BREAK_LATENESS, AXIS_BREAK_AT, 1_000, 0, 300, 800),
            AgentApi::axis_binding(FACT_LOFT_DEMAND, AXIS_LOFT, 1_000, 0, -450, 1_000),
            AgentApi::axis_binding(FACT_PACE_DEMAND, AXIS_PACE, 1_000, 0, 150, 1_000),
        ]);
        let mut memory = AgentApi::empty_memory(1);
        let mut builder = AgentApi::observation_builder(agent, Tick::new(self.steps), 2, 10, 4);
        let _ = builder.add_channel(AgentApi::channel_semantic());
        let _ = builder.add_channel(AgentApi::channel_geometric());
        self.sightings(session)
            .into_iter()
            .for_each(|(kind, value)| {
                let _ = builder.add_fact(AgentApi::observation_fact(
                    kind,
                    0,
                    0,
                    0,
                    0,
                    (value * MILLI) as i64,
                ));
            });
        [1u32, 2, 3].into_iter().for_each(|code| {
            let _ = builder.add_legal_action(code);
        });
        let observation = builder.build();
        let step = RuntimeStep::new(
            FrameIndex::new(self.steps),
            Tick::new(self.steps),
            16_666_667,
            0,
        );
        let (_report, queue) =
            AgentApi::step(agent, profile, &mut brain, &observation, &mut memory, step);
        let axis = |code: u32| queue.axis_value(code) as f32 / MILLI;
        Axes {
            aim_h: axis(AXIS_AIM_H),
            aim_v: axis(AXIS_AIM_V),
            bend: axis(AXIS_BEND),
            break_at: axis(AXIS_BREAK_AT),
            loft: axis(AXIS_LOFT),
            pace: axis(AXIS_PACE),
        }
    }

}

/// The screen mapping for a session, as the agent sees it.
pub fn agent_projection(session: &Session) -> ScreenProjection {
    let tuning = session.tuning();
    let pose = camera::frame(
        AGENT_VIEWPORT,
        &GoalMouth::new(tuning.goal.inset),
        session.shot().origin,
        session.kick().start,
        0.0,
        &tuning.camera,
    );
    ScreenProjection::new(&pose, AGENT_VIEWPORT)
}

/// Play `attempts` penalties headlessly and report `(goals, attempts)`.
///
/// Deterministic, so a run is a reproducible measurement of both the agent and
/// the game's balance — and, because the agent plays by drawing, of the reading
/// as well.
pub fn play_through(seed: u64, attempts: u32, tuning: crate::tuning::Tuning) -> (u32, u32) {
    let mut session = Session::new(tuning);
    let mut striker = Striker::new(seed);
    let mut ticks = 0u32;
    let budget = attempts * 400;
    while (session.tally().attempts < attempts) & (ticks < budget) {
        let projection = agent_projection(&session);
        striker.play(&mut session, &projection);
        ticks += 1;
    }
    (session.tally().goals, session.tally().attempts)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tuning::Tuning;

    #[test]
    fn the_agent_plays_by_drawing_the_same_way_a_finger_does() {
        let mut session = Session::new(Tuning::DEFAULT);
        let mut striker = Striker::new(7);
        let mut seen = Vec::new();
        (0..200).for_each(|_| {
            seen.push(session.phase());
            let projection = agent_projection(&session);
            striker.play(&mut session, &projection);
        });
        [
            Phase::Aiming,
            Phase::ShotReady,
            Phase::Kicking,
            Phase::BallInFlight,
            Phase::Resolution,
        ]
        .iter()
        .for_each(|phase| assert!(seen.contains(phase), "the agent never reached {phase:?}"));
        assert!(session.tally().attempts >= 1, "it took a shot");
    }

    #[test]
    fn what_it_draws_is_a_line_of_pixels_and_nothing_else() {
        let session = Session::new(Tuning::DEFAULT);
        let projection = agent_projection(&session);
        let mut striker = Striker::new(3);
        let line = striker
            .stroke_for(&session, &projection)
            .expect("it draws a line");
        assert!(line.len() >= 3);
        // It is a real drawing: long enough to read, and on the screen.
        assert!(line.length() > AGENT_VIEWPORT.y * 0.2);
        line.points().iter().for_each(|p| {
            assert!(p.x.is_finite() && p.y.is_finite());
        });
        // And the game reads it without knowing where it came from.
        let reading = interpret(
            &line,
            &projection,
            session.shot().origin,
            session.mouth(),
            session.tuning(),
        )
        .expect("the agent's line is readable");
        assert!(reading.intent.target.h.abs() <= 1.0);
    }

    #[test]
    fn the_shot_it_takes_is_the_shot_it_drew() {
        let mut session = Session::new(Tuning::DEFAULT);
        let mut striker = Striker::new(3);
        while session.phase() != Phase::Aiming {
            session.step(&[]);
        }
        let projection = agent_projection(&session);
        let wanted = striker.wanted_shot(&session);
        striker.play(&mut session, &projection);
        assert_eq!(session.phase(), Phase::ShotReady);
        assert!(
            session
                .shot()
                .world_target
                .subtract(wanted.world_target)
                .length()
                < 0.5,
            "it drew at {:?} and the kicker read {:?}",
            wanted.world_target,
            session.shot().world_target
        );
    }

    #[test]
    fn the_agent_scores() {
        // It asks for twelve and gets however many the shootout has room for:
        // the game stops the moment it is decided, and an agent that insisted on
        // taking penalties after that would be playing a different game.
        let (goals, attempts) = play_through(1, 12, Tuning::DEFAULT);
        assert!(attempts > 0 && attempts <= 12, "took {attempts}");
        assert!(goals > 0, "the agent scored nothing in {attempts} attempts");
    }

    #[test]
    fn two_runs_of_the_same_seed_are_the_same_run() {
        assert_eq!(
            play_through(5, 6, Tuning::DEFAULT),
            play_through(5, 6, Tuning::DEFAULT)
        );
    }

    #[test]
    fn it_remembers_which_way_the_keeper_went() {
        let mut session = Session::new(Tuning::DEFAULT);
        let mut striker = Striker::new(2);
        assert_eq!(striker.recall(), None);
        assert_eq!(striker.attempts(), 0);
        let mut ticks = 0;
        while (striker.attempts() == 0) & (ticks < 400) {
            let projection = agent_projection(&session);
            striker.play(&mut session, &projection);
            ticks += 1;
        }
        let recall = striker.recall().expect("it watched its own penalty");
        assert!(recall.aimed.abs() > 0.4, "it aimed at a corner");
        assert_eq!(striker.attempts(), 1);
        // And it only records each attempt once.
        (0..30).for_each(|_| {
            let projection = agent_projection(&session);
            striker.play(&mut session, &projection);
        });
        assert_eq!(striker.attempts(), 1);
    }
}
