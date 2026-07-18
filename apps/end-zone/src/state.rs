//! The deterministic fixed-step simulation orchestrator. One `step` = one
//! 60 Hz tick: apply commands → AI intents → controller movement + contact →
//! ball state machine around one physics step → ordered events. Pure function
//! of `(seed, command stream)`; no wall clock, no ambient randomness.
//!
//! The subsystem stages the orchestrator calls live with their owners: the
//! AI stage in [`crate::ai::stage`], the ball state machine in
//! [`crate::football::sim`], contact resolution in [`crate::player::contact`].

use crate::ai::{
    compile_assignments, AssignmentKind, Perception, PlayerIntent, ResolvedAssignment, RoleState,
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
use crate::player::{contact, controller, PlayerSim};

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
    pub quarterback: PlayerId,
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
            quarterback,
            end_reason: None,
            user_stick: axiom::prelude::Vec2::ZERO,
            perception: Perception::new(),
            events: EventSink::default(),
            rig,
            collision,
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

        self.decide_intents();
        let live = self.phase == PlayPhase::Live;
        controller::integrate_movement(&mut self.players, &self.intents, live, &self.tuning, DT);
        self.collision.resolve(&mut self.players, self.tick);
        self.resolve_contacts();

        self.ball_pre_physics();
        self.rig.mirror_players(&self.players);
        self.rig.step(self.tick);
        self.ball_post_physics();
        self.check_carrier_bounds();

        for player in &mut self.players {
            player.anim_ticks = player.anim_ticks.saturating_add(1);
        }
        self.push_perception();
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
        self.catch_attempted = false;
        self.engaged_blocks.clear();
        if announce {
            self.events
                .emit(SimEvent::PlayStarted { play: self.play.id });
        } else {
            self.events.emit(SimEvent::PlayReset);
        }
    }

    /// Contact resolution: blocks (announced once per pairing per play),
    /// the tackle, and completed falls → ordered events + play end.
    fn resolve_contacts(&mut self) {
        let blocks = contact::resolve_blocks(&mut self.players, &self.intents, &self.tuning);
        for pair in &blocks {
            if !self.engaged_blocks.contains(pair) {
                self.engaged_blocks.push(*pair);
                self.events.emit(SimEvent::BlockEngaged {
                    blocker: pair.0,
                    defender: pair.1,
                });
            }
        }

        let carrier = self.ball.carrier();
        match contact::resolve_tackle(
            &mut self.players,
            &self.intents,
            carrier,
            &self.tuning,
            &self.collision,
        ) {
            Some(outcome) => {
                self.events.emit(SimEvent::TackleContact {
                    tackler: outcome.tackler,
                    target: outcome.target,
                    contact_point: outcome.contact_point,
                    contact_direction: outcome.contact_direction,
                    relative_speed: outcome.relative_speed,
                    strength: outcome.strength,
                    target_airborne: outcome.target_airborne,
                });
                if outcome.target_airborne {
                    self.events.emit(SimEvent::PlayerAirborne {
                        player: outcome.target,
                    });
                }
            }
            // No tackle landed — let close, fast chasers leave their feet.
            None => contact::commit_dives(&mut self.players, &self.intents, carrier, &self.tuning),
        }

        for (player, strength) in contact::advance_falls(&mut self.players, &self.tuning, DT) {
            let position = self.players[player.index()].pos;
            self.events.emit(SimEvent::GroundImpact {
                player,
                position,
                strength,
            });
            if self.ball.carrier() == Some(player) {
                self.end_play(PlayEndReason::Tackled);
            }
        }
    }

    pub(crate) fn end_play(&mut self, reason: PlayEndReason) {
        if self.phase == PlayPhase::Ended {
            return;
        }
        self.phase = PlayPhase::Ended;
        self.end_reason = Some(reason);
        self.events.emit(SimEvent::PlayEnded { reason });
    }
}
