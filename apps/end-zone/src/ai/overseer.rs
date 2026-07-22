//! The `DefensiveOverseer`: an adaptive defensive coordinator for the opponent
//! team. It watches the whole play through the shared perception, runs at a
//! deterministic cadence (never per-player), scores candidate tactical modes,
//! and issues ONE compact [`DefensiveDirective`] the individual player AI
//! executes. It never sets a position, velocity, or steering vector — it changes
//! assignments, emphasis, and responsibilities; players convert those to
//! movement through their existing arbitration.

use crate::config::PLAYER_COUNT;
use crate::football::BallSituation;
use crate::player::PlayerSim;

use super::directive::{AssignmentOverride, DefensiveDirective, TacticalMode};
use super::field_read::{self, DefensiveRead};
use super::perception::PlayPerception;
use super::tactics::{self, ModePlan, DROPBACK_MODES};

/// How often (ticks) the overseer re-scores its tactical read. Player movement
/// still runs every tick; only the strategic call is paced.
const OVERSEER_INTERVAL: u64 = 6;
/// Extra score a challenger needs to unseat the committed mode after its window.
const SWITCH_MARGIN: f32 = 0.2;

/// Lightweight tendencies tracked within the current possession only.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PossessionMemory {
    /// Times the quarterback committed to running this possession.
    pub scramble_events: u32,
    /// Plays the quarterback held the ball a long time.
    pub long_holds: u32,
    /// Deep drop-backs seen.
    pub deep_drops: u32,
    /// Observable targeting evidence per receiver (counted once the ball is in
    /// the air toward him — never a pre-throw read of the intended target).
    pub target_counts: [u32; PLAYER_COUNT],
}

impl PossessionMemory {
    pub fn new() -> Self {
        PossessionMemory {
            scramble_events: 0,
            long_holds: 0,
            deep_drops: 0,
            target_counts: [0; PLAYER_COUNT],
        }
    }
}

impl Default for PossessionMemory {
    fn default() -> Self {
        PossessionMemory::new()
    }
}

/// The opponent defense's coordinator.
#[derive(Debug, Clone)]
pub struct DefensiveOverseer {
    pub directive: DefensiveDirective,
    pub memory: PossessionMemory,
    last_eval: u64,
    snap_tick: u64,
    last_situation: BallSituation,
    prev_mode: TacticalMode,
    transition_reason: &'static str,
    rejected_mode: TacticalMode,
    rejected_score: f32,
    counted_scramble: bool,
    counted_long_hold: bool,
    counted_deep_drop: bool,
    counted_target: bool,
}

impl DefensiveOverseer {
    pub fn new() -> Self {
        DefensiveOverseer {
            directive: DefensiveDirective::base(0),
            memory: PossessionMemory::new(),
            last_eval: 0,
            snap_tick: 0,
            last_situation: BallSituation::PreSnap,
            prev_mode: TacticalMode::Base,
            transition_reason: "init",
            rejected_mode: TacticalMode::Base,
            rejected_score: 0.0,
            counted_scramble: false,
            counted_long_hold: false,
            counted_deep_drop: false,
            counted_target: false,
        }
    }

    /// Reset the per-play directive (a new snap), keeping possession tendencies.
    pub fn reset_play(&mut self, tick: u64) {
        self.directive = DefensiveDirective::base(tick);
        self.last_eval = tick;
        self.last_situation = BallSituation::PreSnap;
        self.snap_tick = tick;
        self.counted_scramble = false;
        self.counted_long_hold = false;
        self.counted_deep_drop = false;
        self.counted_target = false;
    }

    /// Reset possession-level tendency memory at a possession boundary.
    pub fn reset_possession(&mut self) {
        self.memory = PossessionMemory::new();
    }

    pub fn prev_mode(&self) -> TacticalMode {
        self.prev_mode
    }
    pub fn transition_reason(&self) -> &'static str {
        self.transition_reason
    }
    pub fn rejected(&self) -> (TacticalMode, f32) {
        (self.rejected_mode, self.rejected_score)
    }

    /// Advance the overseer one tick and return the active directive. Full
    /// re-scoring happens at the cadence, on a ball-state change, or on an
    /// emergency; between those the committed directive stands.
    pub fn update(
        &mut self,
        tick: u64,
        per: &PlayPerception,
        players: &[PlayerSim],
    ) -> DefensiveDirective {
        // Detect the snap: a live held ball following a dead/pre-snap tick.
        if per.situation != BallSituation::PreSnap
            && self.last_situation == BallSituation::PreSnap
        {
            self.snap_tick = tick;
        }
        let read = field_read::read(per, players, self.snap_tick);
        self.observe(per, &read);

        let situation_changed = per.situation != self.last_situation;
        let forced_switch = tactics::forced_mode(per).is_some_and(|m| m != self.directive.mode);
        let emergency = tactics::is_emergency(per, &read) && self.directive.mode != TacticalMode::EmergencyTouchdown;
        let due = tick.saturating_sub(self.last_eval) >= OVERSEER_INTERVAL;
        if situation_changed || forced_switch || emergency || due {
            self.evaluate(tick, per, &read, players);
            self.last_eval = tick;
        }
        self.last_situation = per.situation;
        self.directive
    }

    /// Accumulate observable possession tendencies (latched once per play each).
    fn observe(&mut self, per: &PlayPerception, read: &DefensiveRead) {
        if per.situation == BallSituation::QbScramble && !self.counted_scramble {
            self.memory.scramble_events += 1;
            self.counted_scramble = true;
        }
        if read.ticks_since_snap > 100 && !self.counted_long_hold {
            self.memory.long_holds += 1;
            self.counted_long_hold = true;
        }
        if read.qb_depth > 5.0 && !self.counted_deep_drop {
            self.memory.deep_drops += 1;
            self.counted_deep_drop = true;
        }
        if per.situation.ball_in_air() && !self.counted_target {
            if let Some(r) = per.intended_receiver {
                self.memory.target_counts[r.index()] += 1;
                self.counted_target = true;
            }
        }
    }

    /// Pick the target plan and switch to it under hysteresis.
    fn evaluate(
        &mut self,
        tick: u64,
        per: &PlayPerception,
        read: &DefensiveRead,
        players: &[PlayerSim],
    ) {
        let target = self.select(per, read);
        if self.should_switch(tick, per, read, &target) {
            self.prev_mode = self.directive.mode;
            self.transition_reason = target.reason;
            self.directive = self.build_directive(tick, &target, per, players);
        }
    }

    /// The best plan for the current read: the forced/emergency mode if one
    /// applies, otherwise the highest-scoring dropback coverage mode.
    fn select(&mut self, per: &PlayPerception, read: &DefensiveRead) -> ModePlan {
        if tactics::is_emergency(per, read) {
            return tactics::plan(TacticalMode::EmergencyTouchdown, per, read, &self.memory);
        }
        if let Some(forced) = tactics::forced_mode(per) {
            return tactics::plan(forced, per, read, &self.memory);
        }
        // Coverage tactics apply only to a live dropback; hold base otherwise
        // (pre-snap, or a dead ball).
        if !field_read::is_dropback(per.situation) {
            return tactics::plan(TacticalMode::Base, per, read, &self.memory);
        }
        let mut plans: Vec<ModePlan> = DROPBACK_MODES
            .iter()
            .map(|&m| tactics::plan(m, per, read, &self.memory))
            .collect();
        plans.sort_by(|a, b| b.score.total_cmp(&a.score));
        // Record the top rejected alternative for the debug view.
        if let Some(second) = plans.get(1) {
            self.rejected_mode = second.mode;
            self.rejected_score = second.score;
        }
        plans[0]
    }

    /// Commitment + hysteresis: keep the current call unless the ball state
    /// forces a change, a touchdown emergency appears, or the window has elapsed
    /// and the challenger is decisively better.
    fn should_switch(
        &self,
        tick: u64,
        per: &PlayPerception,
        read: &DefensiveRead,
        target: &ModePlan,
    ) -> bool {
        if target.mode == self.directive.mode {
            return false;
        }
        let forced = tactics::forced_mode(per) == Some(target.mode)
            || target.mode == TacticalMode::EmergencyTouchdown;
        if forced {
            return true;
        }
        if self.directive.commitment_left(tick) > 0 {
            return false;
        }
        let current = tactics::score(self.directive.mode, read, &self.memory);
        target.score > current + SWITCH_MARGIN
    }

    /// Turn a plan into a directive, assigning defenders to its overrides.
    fn build_directive(
        &self,
        tick: u64,
        plan: &ModePlan,
        per: &PlayPerception,
        players: &[PlayerSim],
    ) -> DefensiveDirective {
        let mut d = DefensiveDirective {
            mode: plan.mode,
            secondary: plan.secondary,
            primary_threat: plan.primary_threat,
            secondary_threat: plan.secondary_threat,
            coverage: plan.coverage,
            shade_side: plan.shade_side,
            rush_emphasis: plan.rush_emphasis,
            coverage_depth: plan.coverage_depth,
            risk_tolerance: plan.risk_tolerance,
            confidence: plan.confidence,
            since_tick: tick,
            min_ticks: plan.min_ticks,
            reason: plan.reason,
            exposed: plan.exposed,
            overrides: [AssignmentOverride::None; PLAYER_COUNT],
        };
        super::personnel::assign_overrides(&mut d, per, players);
        d
    }
}

impl Default for DefensiveOverseer {
    fn default() -> Self {
        DefensiveOverseer::new()
    }
}
