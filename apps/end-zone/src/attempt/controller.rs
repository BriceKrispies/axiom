//! The attempt loop: the one place the game's state machine advances and the
//! only thing that issues simulation commands on the player's behalf.
//!
//! It is stepped once per simulation tick and returns that tick's
//! [`SimCommand`]s — everything below it (AI, ball, contact, the runback moves,
//! presentation) is the app's existing machinery, driven rather than
//! reimplemented.

use crate::data::concept::RUN_LINE;
use crate::events::PlayEndReason;
use crate::identity::PlayerId;
use crate::launch::RunConfig;
use crate::runback::RunbackMove;
use crate::state::{PlayPhase, SimCommand, SimState};

use super::ledger::{AttemptLedger, AttemptOutcome, AttemptRecord};
use super::phase::AttemptPhase;
use super::setup;
use super::{HANDOFF_EARLIEST_TICKS, MAX_LIVE_TICKS, MESH_DEADLINE_TICKS, RESULT_TICKS};

/// The run game's loop.
#[derive(Debug)]
pub struct AttemptController {
    pub(super) phase: AttemptPhase,
    pub(super) ledger: AttemptLedger,
    /// A move latched between simulation ticks. Input arrives once per render
    /// frame and is consumed once per simulation tick; without this latch a move
    /// made on a frame that shares its tick with others would be dropped.
    /// First press of the tick wins — two moves in one tick is a fumbled input,
    /// and honouring the later one would let a stray thumb overwrite a
    /// deliberate press.
    pending: Option<RunbackMove>,
    /// The tick this attempt's play must be dead by.
    dead_at: u64,
    /// Line of scrimmage the attempt snapped from (yards are measured from it).
    los_yard: f32,
    /// Monotonic attempt counter — the defensive variation key's only input
    /// besides the run seed, so coverage varies but never randomly.
    pub(super) attempt_index: u32,
    /// The defensive playbook index this attempt lined up in (inspection).
    pub last_defense_index: usize,
    /// The concept the offense is currently lined up in. It carries into the
    /// next attempt so the offense has somewhere to STAND while the next call is
    /// made — but it is never what gets run: every attempt waits for its own
    /// call.
    pub(super) concept: usize,
    /// A concept picked during this play call, applied when the play installs.
    pub(super) pending_concept: Option<usize>,
}

impl AttemptController {
    /// A fresh loop. The first step builds attempt one, so a run always begins
    /// by lining up rather than in a half-initialized live state.
    pub fn new() -> Self {
        AttemptController {
            phase: AttemptPhase::Resetting,
            ledger: AttemptLedger::new(),
            pending: None,
            dead_at: u64::MAX,
            los_yard: RUN_LINE,
            attempt_index: 0,
            last_defense_index: 0,
            concept: 0,
            pending_concept: None,
        }
    }

    /// Line the first attempt up, so a fresh session is already at the line with
    /// the play card up.
    pub fn arm(&mut self, sim: &mut SimState, config: &RunConfig) {
        self.build_attempt(sim, config);
        self.phase = AttemptPhase::PlayCall;
    }

    pub fn phase(&self) -> AttemptPhase {
        self.phase
    }

    pub fn ledger(&self) -> &AttemptLedger {
        &self.ledger
    }

    /// Time dilation for this tick.
    pub fn time_scale(&self) -> f32 {
        self.phase.time_scale()
    }

    /// Offer the player's move. Accepted only while they actually have the back;
    /// anything else is stale and dropped, so a move mashed during the mesh
    /// cannot fire itself the instant control arrives.
    pub fn command(&mut self, wanted: RunbackMove) -> bool {
        let accepted = self.phase.controllable() && self.pending.is_none();
        self.pending = accepted.then_some(wanted).or(self.pending);
        accepted
    }

    /// Advance one tick and return the simulation commands it implies.
    pub fn step(&mut self, sim: &mut SimState, config: &RunConfig) -> Vec<SimCommand> {
        let tick = sim.tick;
        let mut commands = Vec::new();

        // An ended play preempts every phase.
        if self.phase.is_live() {
            let timed_out = tick >= self.dead_at;
            if timed_out && sim.phase != PlayPhase::Ended {
                sim.blow_dead();
            }
            if sim.phase == PlayPhase::Ended || timed_out {
                self.phase = AttemptPhase::Resolving;
            }
        }

        // The player's move rides the same command stream as everything else,
        // so the simulation cannot tell a human's juke from the agent's.
        if let Some(wanted) = self.pending.take().filter(|_| self.phase.controllable()) {
            commands.push(SimCommand::Runback(wanted));
        }

        self.phase = match self.phase {
            AttemptPhase::Resetting => {
                self.build_attempt(sim, config);
                commands.push(SimCommand::BeginPlay);
                AttemptPhase::PlayCall
            }
            AttemptPhase::PlayCall => self.await_call(sim, config, tick),
            AttemptPhase::Shifting { stalled_at } if self.ready_to_snap(sim, tick, stalled_at) => {
                commands.push(SimCommand::Snap);
                self.dead_at = tick + MAX_LIVE_TICKS;
                AttemptPhase::Mesh { snapped_at: tick }
            }
            AttemptPhase::Shifting { stalled_at } => AttemptPhase::Shifting { stalled_at },
            AttemptPhase::Mesh { snapped_at } => {
                self.mesh(sim, tick, snapped_at, &mut commands)
            }
            AttemptPhase::Exchange => self.exchange(sim),
            AttemptPhase::Carrying => AttemptPhase::Carrying,
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

    /// The quarterback and the back closing on each other. The handoff is
    /// ordered the moment the field says they are together — never on a timer,
    /// so what the player sees (two men meeting) is exactly what happened.
    fn mesh(
        &mut self,
        sim: &SimState,
        tick: u64,
        snapped_at: u64,
        commands: &mut Vec<SimCommand>,
    ) -> AttemptPhase {
        let Some(back) = sim.runback.back else {
            return AttemptPhase::Mesh { snapped_at };
        };
        let ready = tick >= snapped_at + HANDOFF_EARLIEST_TICKS
            && sim.possession == Some(sim.quarterback)
            && sim.mesh_distance(back) <= sim.tuning.handoff_range;
        // Past the deadline the loop stops asking: the quarterback keeps it, and
        // the play resolves however the field decides.
        let give_up = tick >= snapped_at + MESH_DEADLINE_TICKS;
        match (ready, give_up) {
            (true, _) => {
                commands.push(SimCommand::HandOff(back));
                AttemptPhase::Exchange
            }
            (false, true) => AttemptPhase::Carrying,
            (false, false) => AttemptPhase::Mesh { snapped_at },
        }
    }

    /// Wait out the exchange. Control arrives the instant the ball does — and
    /// not before, which is what the whole phase exists to guarantee.
    fn exchange(&mut self, sim: &SimState) -> AttemptPhase {
        let landed = sim
            .runback
            .back
            .map(|back| sim.possession == Some(back))
            .unwrap_or(false);
        // A refused handoff (the two came apart before it completed) falls back
        // rather than stranding the loop in a phase nothing can leave.
        match (landed, sim.ball.is_exchanging()) {
            (true, _) => AttemptPhase::Carrying,
            (false, true) => AttemptPhase::Exchange,
            (false, false) => AttemptPhase::Carrying,
        }
    }

    /// Measure the resolved play and record it.
    fn resolve(&mut self, sim: &SimState) {
        let reason = sim.end_reason.unwrap_or(PlayEndReason::Tackled);
        let back = sim.runback.back.unwrap_or(sim.quarterback);
        let outcome = AttemptOutcome::classify(reason, sim.ball.carrier(), back);
        self.ledger.record(AttemptRecord {
            index: self.attempt_index,
            outcome,
            yards: sim.ball_yard_line() - self.los_yard,
            dodges: sim.runback.dodges,
            broken: sim.runback.broken,
            hurdled: sim.runback.hurdled,
        });
    }

    /// The running back for the installed play (inspection + the agent).
    pub fn back(&self, sim: &SimState) -> Option<PlayerId> {
        sim.runback.back
    }

    /// Build the next attempt. Every piece of per-attempt state is reset HERE
    /// and nowhere else, so a stale move, a stale clock or a stale success
    /// cannot survive into the next attempt however the last one ended.
    fn build_attempt(&mut self, sim: &mut SimState, config: &RunConfig) {
        self.attempt_index += 1;
        self.pending = None;
        self.dead_at = u64::MAX;
        self.los_yard = RUN_LINE;
        self.pending_concept = None;
        self.last_defense_index = setup::install(sim, config, self.attempt_index, self.concept);
    }
}

impl Default for AttemptController {
    fn default() -> Self {
        AttemptController::new()
    }
}
