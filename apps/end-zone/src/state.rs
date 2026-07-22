//! The deterministic fixed-step simulation orchestrator. One `step` = one
//! 60 Hz tick: apply commands → AI intents → controller movement + contact →
//! ball state machine around one physics step → ordered events. Pure function
//! of `(seed, command stream)`; no wall clock, no ambient randomness.
//!
//! The subsystem stages the orchestrator calls live with their owners: the
//! AI stage in [`crate::ai::stage`], the ball state machine in
//! [`crate::football::sim`], contact resolution in [`crate::player::contact`].

use crate::ai::engagement::EngagementLink;
use crate::ai::{
    compile_assignments, AiMemory, AssignmentKind, DefensiveOverseer, Perception, PlayerIntent,
    ResolvedAssignment, RoleState,
};
use crate::collision_rig::CollisionRig;
use crate::config::{EndZoneConfig, DT, PLAYER_COUNT};
use crate::data::{
    showcase_play, showcase_rosters, BehaviorTuning, PlayDefinition, RosterDefinition,
};
use crate::events::{EventSink, PlayEndReason, SimEvent, StampedEvent};
use crate::field::{OffenseFrame, OffensePoint};
use crate::football::sim::ball_rest;
use crate::football::BallSim;
use crate::identity::PlayerId;
use crate::physics_rig::PhysicsRig;
use crate::player::lineup::formation_players;
use crate::player::{controller, PlayerSim};

/// The play lifecycle.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlayPhase {
    PreSnap,
    Live,
    Ended,
}

/// Commands the simulation accepts (issued by the showcase controller and the
/// diagnostic input; there are no gameplay controls yet).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SimCommand {
    /// (Re)set the play to formation and mark it started.
    BeginPlay,
    /// Snap the ball.
    Snap,
    /// Order the quarterback to throw the scripted pass.
    ThrowNow,
    /// Reset to formation without starting (diagnostic R).
    ResetPlay,
}

/// The authoritative simulation state.
#[derive(Debug)]
pub struct SimState {
    pub seed: u64,
    pub tuning: BehaviorTuning,
    pub tick: u64,
    pub phase: PlayPhase,
    pub play: PlayDefinition,
    pub frame: OffenseFrame,
    /// The roster DATA every (re)formation is built from — mutate these to
    /// change behavior without touching a line of system code.
    pub rosters: (RosterDefinition, RosterDefinition),
    pub players: Vec<PlayerSim>,
    pub assignments: Vec<ResolvedAssignment>,
    pub roles: Vec<RoleState>,
    /// Last tick's intents (AI stage output — inspectable + replay-compared).
    pub intents: Vec<PlayerIntent>,
    pub ball: BallSim,
    pub possession: Option<PlayerId>,
    /// The tick the current carrier gained possession (drives the catch-secure
    /// window before he can be tackled).
    pub(crate) possession_since: u64,
    pub quarterback: PlayerId,
    /// Persistent AI memory: per-player commitments, the scramble counter, the
    /// derived situation, and this tick's coordinated responsibilities.
    pub(crate) ai_memory: AiMemory,
    /// Per-blocker line engagements (indexed by blocker id): written by the
    /// contact stage, read by the AI the next tick.
    pub(crate) engagements: Vec<EngagementLink>,
    /// The opponent defense's adaptive coordinator (team-level directives).
    pub(crate) overseer: DefensiveOverseer,
    pub end_reason: Option<PlayEndReason>,
    /// The user's movement stick for this tick, offense-relative
    /// (`x` = offense right, `y` = downfield), each in `-1..=1`. Part of the
    /// deterministic input stream: zero when no one is steering, in which
    /// case the AI intent stands untouched.
    pub user_stick: axiom::prelude::Vec2,
    pub(crate) perception: Perception,
    pub(crate) events: EventSink,
    pub(crate) rig: PhysicsRig,
    /// Player-vs-player contact solver (real rigid-body de-penetration +
    /// momentum exchange) — replaces the old positional `resolve_overlaps`.
    pub(crate) collision: CollisionRig,
    /// The receiver the in-flight wind-up committed to, locked when the
    /// wind-up begins and cleared at release. `None` outside a wind-up.
    pub(crate) throw_target: Option<PlayerId>,
    /// This tick's eligible receivers — everyone inside the quarterback's
    /// throwing cone, nearest the centre line first. Empty unless the
    /// quarterback is holding a live ball. Presentation draws a ring at each
    /// one's feet; the sim throws to the first.
    pub throwable: Vec<PlayerId>,
    pub(crate) throw_commanded: bool,
    pub(crate) catch_attempted: bool,
    pub(crate) engaged_blocks: Vec<(PlayerId, PlayerId)>,
}

impl SimState {
    /// A fresh showcase simulation in formation, pre-snap (the default
    /// showcase matchup: league slots 0 and 1, Pro tuning).
    pub fn new(config: EndZoneConfig) -> Self {
        SimState::new_match(&crate::launch::RunSetup {
            rosters: showcase_rosters(),
            tuning: BehaviorTuning::default(),
            seed: config.seed,
        })
    }

    /// A fresh simulation for a resolved match setup: rosters already in sim
    /// slots (player = possession slot 0), tuning already profiled — the ONE
    /// bootstrap the launch boundary feeds.
    pub fn new_match(setup: &crate::launch::RunSetup) -> Self {
        let play = showcase_play();
        let tuning = setup.tuning;
        let frame = OffenseFrame::at_yard_line(play.line_of_scrimmage, play.drive_direction);
        let assignments = compile_assignments(&play, &frame);
        let quarterback = assignments
            .iter()
            .enumerate()
            .find(|(_, a)| matches!(a.kind, AssignmentKind::Quarterback { .. }))
            .map(|(i, _)| PlayerId(i as u8))
            .unwrap_or(PlayerId(0));
        let rosters = setup.rosters.clone();
        let players = formation_players(&play, &frame, &rosters);
        let ball_spawn = frame.to_world(OffensePoint::new(0.0, 0.0)).add(ball_rest());
        let mut rig = PhysicsRig::new(tuning.gravity, ball_spawn);
        rig.park_ball();
        let collision = CollisionRig::new(&players);
        let mut sim = SimState {
            seed: setup.seed,
            tuning,
            tick: 0,
            phase: PlayPhase::PreSnap,
            play,
            frame,
            rosters,
            players,
            assignments,
            roles: vec![RoleState::Waiting; PLAYER_COUNT],
            intents: vec![PlayerIntent::Hold; PLAYER_COUNT],
            ball: BallSim::dead_at(ball_spawn),
            possession: None,
            possession_since: 0,
            quarterback,
            ai_memory: AiMemory::new(),
            engagements: vec![None; PLAYER_COUNT],
            overseer: DefensiveOverseer::new(),
            end_reason: None,
            user_stick: axiom::prelude::Vec2::ZERO,
            perception: Perception::new(),
            events: EventSink::default(),
            rig,
            collision,
            throw_target: None,
            throwable: Vec::new(),
            throw_commanded: false,
            catch_attempted: false,
            engaged_blocks: Vec::new(),
        };
        sim.push_perception();
        sim
    }

    /// Advance one fixed step under `commands`, returning this tick's events.
    pub fn step(&mut self, commands: &[SimCommand]) -> &[StampedEvent] {
        self.events.begin_tick(self.tick);
        for command in commands {
            self.apply_command(*command);
        }
        let prev_possession = self.possession;

        self.decide_intents();
        let phase = self.phase;
        controller::integrate_movement(&mut self.players, &self.intents, phase, &self.tuning, DT);
        self.collision.resolve(&mut self.players, self.tick);
        self.resolve_contacts();

        self.ball_pre_physics();
        self.rig.mirror_players(&self.players);
        self.rig.step(self.tick);
        self.ball_post_physics();
        self.check_carrier_bounds();
        if self.possession.is_some() && self.possession != prev_possession {
            self.possession_since = self.tick;
        }

        for player in &mut self.players {
            player.anim_ticks = player.anim_ticks.saturating_add(1);
        }
        self.push_perception();
        self.update_throwable();
        self.throw_commanded = false;
        self.tick += 1;
        self.events.events()
    }

    /// This tick's ordered events.
    pub fn events(&self) -> &[StampedEvent] {
        self.events.events()
    }

    /// First recorded physics fault, if any (debug overlay row) — from either
    /// the ball rig or the player-collision rig.
    pub fn fault(&self) -> Option<&'static str> {
        self.rig.fault.or(self.collision.fault)
    }

    // The deterministic state digest lives with the other replay artifacts
    // in `crate::trace` (`SimState::digest`).

    fn apply_command(&mut self, command: SimCommand) {
        match command {
            SimCommand::BeginPlay => self.reset_to_formation(true),
            SimCommand::Snap => self.snap(),
            SimCommand::ThrowNow => self.throw_commanded = true,
            SimCommand::ResetPlay => self.reset_to_formation(false),
        }
    }

    /// Put everything back in formation. `announce` emits `PlayStarted`
    /// (a bare reset emits `PlayReset`).
    pub fn reset_to_formation(&mut self, announce: bool) {
        self.players = formation_players(&self.play, &self.frame, &self.rosters);
        self.roles = vec![RoleState::Waiting; PLAYER_COUNT];
        self.intents = vec![PlayerIntent::Hold; PLAYER_COUNT];
        self.ai_memory.reset();
        self.engagements = vec![None; PLAYER_COUNT];
        self.overseer.reset_play(self.tick);
        let spawn = self
            .frame
            .to_world(OffensePoint::new(0.0, 0.0))
            .add(ball_rest());
        self.ball = BallSim::dead_at(spawn);
        self.rig.park_ball();
        self.possession = None;
        self.phase = PlayPhase::PreSnap;
        self.end_reason = None;
        self.throw_commanded = false;
        self.throw_target = None;
        self.throwable.clear();
        self.catch_attempted = false;
        self.engaged_blocks.clear();
        if announce {
            self.events
                .emit(SimEvent::PlayStarted { play: self.play.id });
        } else {
            self.events.emit(SimEvent::PlayReset);
        }
    }

    pub(crate) fn end_play(&mut self, reason: PlayEndReason) {
        if self.phase == PlayPhase::Ended {
            return;
        }
        self.phase = PlayPhase::Ended;
        self.end_reason = Some(reason);
        // Crossing the goal is a score — the possession boundary at which the
        // overseer's tendency memory resets.
        (reason == PlayEndReason::BrokeFree).then(|| self.overseer.reset_possession());
        self.events.emit(SimEvent::PlayEnded { reason });
    }
}
