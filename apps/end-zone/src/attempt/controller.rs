//! The attempt loop: the one place the prototype's state machine advances and
//! the only thing that issues simulation commands on the player's behalf.
//!
//! It is stepped once per simulation tick and returns that tick's
//! [`SimCommand`]s, exactly like the old drive controller did — everything
//! below it (AI, ball, contact, presentation) is the app's existing machinery,
//! untouched.

use crate::data::prototype::PROTOTYPE_LINE;
use crate::events::PlayEndReason;
use crate::launch::RunConfig;
use crate::state::{PlayPhase, SimCommand, SimState};

use super::ledger::{AttemptLedger, AttemptOutcome, AttemptRecord};
use super::phase::{window_length, AttemptPhase, PlayerChoice};
use super::read::{read_play, window_trigger, PlayRead, WindowGate};
use super::setup;
use super::{
    DEVELOP_MAX_TICKS, DEVELOP_MIN_TICKS, MAX_LIVE_TICKS, REARM_DEADLINE_TICKS, RESULT_TICKS,
    SET_TICKS, WINDOW_COOLDOWN_TICKS,
};

/// The prototype's run loop.
#[derive(Debug)]
pub struct AttemptController {
    pub(super) phase: AttemptPhase,
    pub(super) ledger: AttemptLedger,
    gate: WindowGate,
    pub(super) read: Option<PlayRead>,
    /// The choice the player committed to this attempt.
    choice: Option<PlayerChoice>,
    /// A press latched between simulation ticks. Input arrives once per render
    /// frame, and in slow motion many render frames share one tick — without
    /// this latch a decision made mid-dilation would be dropped.
    pending: Option<PlayerChoice>,
    /// Windows offered this attempt.
    pub(super) windows: u32,
    /// The tick this attempt's play must be dead by.
    dead_at: u64,
    /// Line of scrimmage the attempt snapped from (yards are measured from it).
    los_yard: f32,
    /// Monotonic attempt counter — the defensive variation key.s only input
    /// besides the run seed, so coverage varies but never randomly.
    pub(super) attempt_index: u32,
    /// The defensive playbook index this attempt lined up in (inspection).
    pub last_defense_index: usize,
    /// The concept the offense runs. Chosen pre-snap and CARRIED into the next
    /// attempt, so a player who likes a concept keeps it rather than re-picking
    /// every eight seconds.
    pub(super) concept: usize,
    /// A concept picked during this pre-snap, applied when the play installs.
    pub(super) pending_concept: Option<usize>,
}

impl AttemptController {
    /// A fresh loop. The first step builds attempt one, so a run always begins
    /// by lining up rather than in a half-initialized live state.
    pub fn new() -> Self {
        AttemptController {
            phase: AttemptPhase::Resetting,
            ledger: AttemptLedger::new(),
            gate: WindowGate::closed(),
            read: None,
            choice: None,
            pending: None,
            windows: 0,
            dead_at: u64::MAX,
            los_yard: PROTOTYPE_LINE,
            attempt_index: 0,
            last_defense_index: 0,
            concept: 0,
            pending_concept: None,
        }
    }

    /// Line the first attempt up, so a fresh session is already set at the line
    /// (without it there is no attempt view at tick zero to draw from).
    pub fn arm(&mut self, sim: &mut SimState, config: &RunConfig) {
        self.build_attempt(sim, config);
        self.read = Some(read_play(sim, self.concept));
        self.phase = AttemptPhase::PreSnap {
            snap_at: sim.tick + SET_TICKS,
        };
    }

    pub fn phase(&self) -> AttemptPhase {
        self.phase
    }

    pub fn ledger(&self) -> &AttemptLedger {
        &self.ledger
    }

    /// Time dilation for this tick (the decision window's slow motion).
    pub fn time_scale(&self) -> f32 {
        self.phase.time_scale()
    }

    /// Offer the player's choice. Accepted only while the reads are live and
    /// nothing has been decided yet — anything else is stale and is dropped, so
    /// a mashed button cannot overwrite a decision already made. The latch
    /// counts as decided.
    pub fn choose(&mut self, choice: PlayerChoice) -> bool {
        let accepted =
            self.phase.accepts_choice() && self.choice.is_none() && self.pending.is_none();
        self.pending = accepted.then_some(choice).or(self.pending);
        accepted
    }

    /// Advance one tick and return the simulation commands it implies.
    pub fn step(&mut self, sim: &mut SimState, config: &RunConfig) -> Vec<SimCommand> {
        let tick = sim.tick;
        let read = read_play(sim, self.concept);
        self.read = Some(read);
        let mut commands = Vec::new();

        // A play the simulation (or the attempt clock) has ended preempts every
        // phase — a sack DURING the decision window is exactly the "waited too
        // long" outcome the prototype is built to produce.
        if self.is_live() {
            let timed_out = tick >= self.dead_at;
            if timed_out && sim.phase != PlayPhase::Ended {
                sim.blow_dead();
            }
            if sim.phase == PlayPhase::Ended || timed_out {
                self.phase = AttemptPhase::Resolving;
            }
        }

        self.phase = match self.phase {
            AttemptPhase::Resetting => {
                self.build_attempt(sim, config);
                commands.push(SimCommand::BeginPlay);
                AttemptPhase::PreSnap {
                    snap_at: tick + SET_TICKS,
                }
            }
            AttemptPhase::PreSnap { snap_at } if tick >= snap_at => {
                commands.push(SimCommand::Snap);
                self.dead_at = tick + MAX_LIVE_TICKS;
                self.gate = WindowGate {
                    armed_at: tick + DEVELOP_MIN_TICKS,
                    deadline: tick + DEVELOP_MAX_TICKS,
                    windows_used: 0,
                    last_best: None,
                };
                AttemptPhase::Developing
            }
            AttemptPhase::PreSnap { snap_at } => {
                // Applying the pick RE-INSTALLS the play, which recompiles the
                // route waypoints and re-lines the offense up. Without that the
                // picker would only relabel the reads while the receivers ran
                // whatever concept was installed at reset.
                if let Some(next) = self.pending_concept.take() {
                    self.concept = next;
                    self.last_defense_index =
                        setup::install(sim, config, self.attempt_index, self.concept);
                    commands.push(SimCommand::BeginPlay);
                }
                AttemptPhase::PreSnap { snap_at }
            }
            // A choice can land here as well as in a window: throwing early, at
            // full speed, is the anticipatory read.
            AttemptPhase::Developing => match self.pending.take() {
                Some(choice) => self.commit(&read, choice, &mut commands),
                None => self.maybe_open_window(&read, tick),
            },
            AttemptPhase::DecisionWindow {
                opened_at,
                closes_at,
                trigger,
            } => match self.pending.take() {
                Some(choice) => self.commit(&read, choice, &mut commands),
                None if tick >= closes_at => self.decline(tick),
                None => AttemptPhase::DecisionWindow {
                    opened_at,
                    closes_at,
                    trigger,
                },
            },
            AttemptPhase::PassInFlight { read } => AttemptPhase::PassInFlight { read },
            AttemptPhase::Scrambling => AttemptPhase::Scrambling,
            AttemptPhase::Resolving => {
                self.resolve(sim);
                AttemptPhase::Result {
                    until: tick + RESULT_TICKS,
                }
            }
            AttemptPhase::Result { until } if tick >= until => AttemptPhase::Resetting,
            AttemptPhase::Result { until } => AttemptPhase::Result { until },
        };
        commands
    }

    /// Whether the play underneath is running.
    fn is_live(&self) -> bool {
        matches!(
            self.phase,
            AttemptPhase::Developing
                | AttemptPhase::DecisionWindow { .. }
                | AttemptPhase::PassInFlight { .. }
                | AttemptPhase::Scrambling
        )
    }

    /// Open a window if this tick earns one.
    fn maybe_open_window(&mut self, read: &PlayRead, tick: u64) -> AttemptPhase {
        let Some(trigger) = window_trigger(read, tick, &self.gate) else {
            return AttemptPhase::Developing;
        };
        let length = window_length(self.gate.windows_used);
        self.gate.windows_used += 1;
        self.gate.last_best = Some(read.best);
        self.windows += 1;
        AttemptPhase::DecisionWindow {
            opened_at: tick,
            closes_at: tick + length,
            trigger,
        }
    }

    /// The player let the window close. Full speed resumes, the rush keeps
    /// coming, and the next look is armed — shorter, and later.
    fn decline(&mut self, tick: u64) -> AttemptPhase {
        self.gate.armed_at = tick + WINDOW_COOLDOWN_TICKS;
        self.gate.deadline = tick + REARM_DEADLINE_TICKS;
        AttemptPhase::Developing
    }

    /// Turn a choice into simulation commands.
    fn commit(
        &mut self,
        read: &PlayRead,
        choice: PlayerChoice,
        commands: &mut Vec<SimCommand>,
    ) -> AttemptPhase {
        self.choice = Some(choice);
        match choice {
            PlayerChoice::Throw(target) => {
                commands.push(SimCommand::ThrowTo(read.target(target)));
                AttemptPhase::PassInFlight { read: target }
            }
            // The wind-up already queued its own release; issuing a throw here
            // too would overwrite the player's charge with full power.
            PlayerChoice::ThrowCharged(target) => AttemptPhase::PassInFlight { read: target },
            PlayerChoice::Scramble => {
                commands.push(SimCommand::Scramble);
                AttemptPhase::Scrambling
            }
        }
    }

    /// Measure the resolved play and record it.
    fn resolve(&mut self, sim: &SimState) {
        let reason = sim.end_reason.unwrap_or(PlayEndReason::Incomplete);
        let scrambled = self.choice == Some(PlayerChoice::Scramble);
        let outcome = AttemptOutcome::classify(
            reason,
            self.choice.and_then(|c| c.read()),
            scrambled,
            sim.ball.carrier(),
            sim.quarterback,
        );
        // Yards always come from where the play actually ended. A dead ball the
        // offense never possessed (incompletion, interception) moves nothing.
        let yards = match outcome {
            AttemptOutcome::Incomplete | AttemptOutcome::Intercepted => 0.0,
            _ => sim.ball_yard_line() - self.los_yard,
        };
        self.ledger.record(AttemptRecord {
            index: self.attempt_index,
            outcome,
            yards,
            read: self.choice.and_then(|c| c.read()),
            windows: self.windows,
            declined: self.choice.is_none() && self.windows > 0,
        });
    }

    /// Build the next attempt. Every piece of per-attempt state is reset HERE
    /// and nowhere else, so a stale window, a stale choice or a stale clock
    /// cannot survive into the next attempt however the last one ended.
    fn build_attempt(&mut self, sim: &mut SimState, config: &RunConfig) {
        self.attempt_index += 1;
        self.choice = None;
        self.pending = None;
        self.windows = 0;
        self.dead_at = u64::MAX;
        self.los_yard = PROTOTYPE_LINE;
        self.gate = WindowGate::closed();
        self.last_defense_index = setup::install(sim, config, self.attempt_index, self.concept);
    }
}

impl Default for AttemptController {
    fn default() -> Self {
        AttemptController::new()
    }
}
