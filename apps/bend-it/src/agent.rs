//! An embodied agent that plays Bend It, through the reusable `axiom-agent`
//! substrate.
//!
//! The game is playable by a machine for the same reason it is playable by a
//! thumb: what the player *says* is a five-word command vocabulary, and a shot
//! is four scalars and a commit. So the agent does not need a second interface,
//! a cheat hook, or a scripted sequence — it says exactly what a finger says.
//!
//! ```text
//! session state --perceive--> Observation (integer facts)   the striker's eyes
//!               --axiom-agent decide--> move_axis intents   the striker's hands
//!               --lower--> EditorCommand --> session.step()
//! ```
//!
//! # What is the agent's, and what is the app's
//!
//! **The app owns perception.** Which corner is open, whether the keeper went
//! the right way last time, how late a curve has to break — every part of that
//! names a Bend It noun (a goal mouth, a keeper's dive, a bend curve) that
//! `axiom-agent` must never learn.
//!
//! **The agent owns the control law.** A table of neutral bindings, each turning
//! a perceived scalar into a deflection of a control axis with a gain and
//! limits. It contains no soccer at all; the same table shape would fly a plane.
//! Nothing here hand-rolls the decision — every aim, every bend and every loft is
//! emitted by `AgentApi::step` as a `move_axis` intent and lowered back into the
//! one command the session reads. Cut the agent out and the ball is never struck.
//!
//! # How it tries to score
//!
//! It remembers one thing: which way the keeper went last time, and whether that
//! shot went in. Then it attacks the *other* side, breaks the curve late enough
//! that the keeper's one mid-flight correction cannot answer it, and keeps
//! repeating a shape that worked. That is a striker's whole reasoning, and it is
//! four facts.

use axiom_agent::AgentApi;
use axiom_kernel::{FrameIndex, Tick};
use axiom_runtime::RuntimeStep;

use crate::play::{EditorCommand, Phase, Session, ShotResult};
use crate::shot::{BendCurve, GoalTarget};

/// The app's control-axis vocabulary: the meaning this app assigns to a neutral
/// `move_axis` code. `axiom-agent` carries the `u32` opaquely.
pub const AXIS_AIM_H: u32 = 1;
pub const AXIS_AIM_V: u32 = 2;
pub const AXIS_BEND: u32 = 3;
pub const AXIS_BREAK_AT: u32 = 4;
pub const AXIS_LOFT: u32 = 5;

/// The app's observation-fact vocabulary: what the striker can *see*. Values are
/// milli-units, because agent facts are integer only.
pub const FACT_OPEN_SIDE: u16 = 10;
pub const FACT_OPEN_HEIGHT: u16 = 11;
pub const FACT_BEND_DEMAND: u16 = 12;
pub const FACT_BREAK_LATENESS: u16 = 13;
pub const FACT_LOFT_DEMAND: u16 = 14;

/// One milli-unit.
const MILLI: f32 = 1000.0;

/// What the striker remembers about the last attempt: which way it aimed, which
/// way the keeper went, and whether it went in.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Recall {
    pub aimed: f32,
    pub keeper_went: f32,
    pub scored: bool,
}

/// The striker.
///
/// It holds only *app* state — what it saw, and how many penalties it has taken.
/// The agent itself is built fresh inside each decision, because every contract
/// type in `axiom-agent` is sealed behind its one facade and cannot be named in a
/// struct field. That is the Module Law working as intended rather than an
/// inconvenience: the binding table is a pure value, the brain is stateless, and
/// the only thing that genuinely persists between decisions is a soccer memory,
/// which is the app's to keep.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Striker {
    seed: u64,
    recall: Option<Recall>,
    attempt: u32,
    steps: u64,
    /// Whether this attempt's result has already been folded into the recall.
    recorded: bool,
}

impl Striker {
    /// A striker. `seed` varies which corner it opens with, so two strikers are
    /// not the same striker.
    pub fn new(seed: u64) -> Striker {
        Striker {
            seed,
            recall: None,
            attempt: 0,
            steps: 0,
            recorded: false,
        }
    }

    /// What it remembers.
    pub fn recall(&self) -> Option<Recall> {
        self.recall
    }

    /// How many attempts it has taken.
    pub fn attempts(&self) -> u32 {
        self.attempt
    }

    /// Play one tick: perceive, decide, lower, step.
    pub fn play(&mut self, session: &mut Session) {
        self.remember(session);
        let commands = self.decide(session);
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
        // A fresh attempt re-arms the recording.
        self.recorded &= session.result().is_some();
    }

    /// Perceive, decide, and lower into the commands this phase accepts.
    fn decide(&mut self, session: &Session) -> Vec<EditorCommand> {
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
        ]);
        let mut memory = AgentApi::empty_memory(1);
        // The observation is assembled here rather than returned from a helper
        // for the same Module-Law reason the brain is: `Observation` is sealed
        // behind the facade. What IS the app's — what the striker can see — is a
        // plain list of scalars, and that is what `sightings` returns.
        let mut builder = AgentApi::observation_builder(agent, Tick::new(self.steps), 2, 8, 4);
        let _ = builder.add_channel(AgentApi::channel_semantic());
        let _ = builder.add_channel(AgentApi::channel_geometric());
        self.sightings(session).into_iter().for_each(|(kind, value)| {
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
        let (_report, queue) = AgentApi::step(
            agent,
            profile,
            &mut brain,
            &observation,
            &mut memory,
            step,
        );
        let axis = |code: u32| queue.axis_value(code) as f32 / MILLI;
        let tuning = session.tuning();
        match session.phase() {
            Phase::TargetSelection => vec![
                EditorCommand::Aim(GoalTarget::new(axis(AXIS_AIM_H), axis(AXIS_AIM_V))),
                EditorCommand::Advance,
            ],
            Phase::HorizontalSculpt => vec![
                EditorCommand::SetBend(BendCurve::through(
                    axis(AXIS_BREAK_AT),
                    axis(AXIS_BEND) * tuning.bend.max_offset,
                    tuning.bend.peak_margin,
                )),
                EditorCommand::Advance,
            ],
            Phase::VerticalSculpt => vec![
                EditorCommand::SetLoft(BendCurve::through(
                    axis(AXIS_BREAK_AT),
                    axis(AXIS_LOFT) * tuning.loft.max_offset,
                    tuning.loft.peak_margin,
                )),
                EditorCommand::Advance,
            ],
            _ => Vec::new(),
        }
    }

    /// The striker's eyes: what it can see this tick, as `(fact kind, scalar)`.
    ///
    /// Four sightings, and every one of them is a Bend It noun reduced to a
    /// number: which side of the goal is open, how high in it, how much the shot
    /// has to bend to get there, and how late that bend has to break. This is
    /// perception, so it is the app's, and it is a pure function of what the
    /// striker has watched.
    pub fn sightings(&self, session: &Session) -> [(u16, f32); 5] {
        // Which side is open. A keeper that went one way is not going the other,
        // and a shape that scored is worth repeating; anything else, switch. With
        // nothing yet remembered, it takes its cue from where the keeper is
        // standing right now and attacks the side it is further from.
        let standing = session.keeper().motion().hips.x;
        let side = self
            .recall
            .map(|r| match r.scored {
                true => nonzero_sign(r.aimed),
                false => -nonzero_sign(r.keeper_went),
            })
            .unwrap_or_else(|| -nonzero_sign(standing + self.opening_bias()))
            * 0.94;
        // Low, and arced to get there. A keeper reads the first fraction of the
        // flight: a ball still climbing at that moment is read as arriving high,
        // and a keeper that has thrown its hands up cannot get them back down.
        // The pair only works together — a flat shot to the same low corner is
        // read correctly and saved.
        let height = 0.15;
        let loft = 0.52;
        // Bend AWAY from the side being attacked, so the ball leaves toward the
        // half the keeper is being sent to and finishes in the other one — but
        // only about half of what the editor allows. Over-bending is punished:
        // a shot bent to its limit swings wide and then comes back *through* the
        // dive it created.
        let bend = -nonzero_sign(side) * 0.48;
        // Break it late. Movement before the keeper's correction is movement the
        // keeper answers; movement after it is movement it cannot.
        let lateness = 0.72;
        [
            (FACT_OPEN_SIDE, side),
            (FACT_OPEN_HEIGHT, height),
            (FACT_BEND_DEMAND, bend),
            (FACT_BREAK_LATENESS, lateness),
            (FACT_LOFT_DEMAND, loft),
        ]
    }

    /// The one bit of the striker's identity its opening corner depends on, so
    /// two seeds are two different players rather than two copies of one.
    fn opening_bias(&self) -> f32 {
        [0.01f32, -0.01][(self.seed % 2) as usize]
    }
}

/// `signum`, but never zero — a striker with no preference still has to pick a
/// side.
fn nonzero_sign(v: f32) -> f32 {
    [1.0f32, -1.0][usize::from(v < 0.0)]
}

/// Play `attempts` penalties headlessly and report `(goals, attempts)`.
///
/// This is the whole agent play-through: a fresh session, a striker, and enough
/// ticks for it to take its shots. It is deterministic, so a run is a
/// reproducible measurement of both the agent and the game's balance.
pub fn play_through(seed: u64, attempts: u32, tuning: crate::tuning::Tuning) -> (u32, u32) {
    let mut session = Session::new(tuning);
    let mut striker = Striker::new(seed);
    let mut ticks = 0u32;
    // Each attempt is a settle, three edits, a commit, a run-up, a flight, a
    // banner and a reset — comfortably inside 400 ticks.
    let budget = attempts * 400;
    while (session.tally().attempts < attempts) & (ticks < budget) {
        striker.play(&mut session);
        ticks += 1;
    }
    (session.tally().goals, session.tally().attempts)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tuning::Tuning;

    #[test]
    fn the_agent_plays_the_game_through_the_same_commands_a_thumb_does() {
        let mut session = Session::new(Tuning::DEFAULT);
        let mut striker = Striker::new(7);
        // It walks the whole flow on its own: aim, bend, height, kick.
        let mut seen = Vec::new();
        (0..200).for_each(|_| {
            seen.push(session.phase());
            striker.play(&mut session);
        });
        [
            Phase::TargetSelection,
            Phase::HorizontalSculpt,
            Phase::VerticalSculpt,
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
    fn the_agent_authors_a_real_shot_and_never_an_illegal_one() {
        let mut session = Session::new(Tuning::DEFAULT);
        let mut striker = Striker::new(3);
        (0..90).for_each(|_| striker.play(&mut session));
        let intent = session.intent();
        assert!(intent.target.h.abs() <= 1.0 && (0.0..=1.0).contains(&intent.target.v));
        assert!(intent.bend.magnitude().abs() > 0.4, "it actually bends the shot");
        // Whatever it authored, the invariants still hold.
        let points = session.shot().trajectory.points();
        assert_eq!(points[0], session.shot().origin);
        assert_eq!(*points.last().expect("a path"), session.shot().world_target);
        assert!(points.iter().all(|p| p.y >= Tuning::DEFAULT.flight.ball_radius - 1.0e-4));
    }

    #[test]
    fn the_agent_scores() {
        let (goals, attempts) = play_through(1, 12, Tuning::DEFAULT);
        assert_eq!(attempts, 12, "it finished every attempt");
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
            striker.play(&mut session);
            ticks += 1;
        }
        let recall = striker.recall().expect("it watched its own penalty");
        assert!(recall.aimed.abs() > 0.5, "it aimed at a corner");
        assert_eq!(striker.attempts(), 1);
        // And it only records each attempt once.
        (0..30).for_each(|_| striker.play(&mut session));
        assert_eq!(striker.attempts(), 1);
    }
}
