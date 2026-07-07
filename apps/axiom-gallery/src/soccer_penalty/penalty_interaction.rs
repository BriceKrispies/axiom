//! Pass 4 — the deterministic aim + shot-power interaction model.
//!
//! A tiny fixed-tick state machine driven purely by [`PenaltyInputIntent`]. It
//! moves an aim reticle inside the goal-mouth target rectangle and charges a
//! shot-power meter, then freezes both into a [`PenaltyShotPreview`] on release.
//! **The ball never moves here** — this is aim/power intent only; ball flight,
//! saves, and scoring are later passes.
//!
//! Everything is integer arithmetic over fixed constants and advances one tick
//! per [`PenaltyInteractionState::advance`] call: no wall-clock time, no
//! randomness, no unordered iteration. Identical intent sequences yield
//! byte-identical states.
//!
//! Pass 5 extends the machine past `LockedPreview` into `BallInFlight` and
//! `ArrivedAtGoalPlane`: on the tick after a lock the ball launches and then
//! advances deterministically along its [`crate::soccer_penalty::penalty_ball`] trajectory. No
//! goal/save/miss/post is resolved.

use crate::soccer_penalty::penalty_ball::{resting_pose, PenaltyBallFlight, PenaltyBallPose, PenaltyBallState};
use crate::soccer_penalty::penalty_goalie::{PenaltyGoalieContactDetector, PenaltyGoalieContactFrame};
use crate::soccer_penalty::penalty_goalie_pose::PenaltyGoalieAnimation;
use crate::soccer_penalty::penalty_input::PenaltyInputIntent;
use crate::soccer_penalty::penalty_result::{
    PenaltyGoalPlaneCrossing, PenaltyResolvedShotState, PenaltyShotResultResolver,
};

/// Aim reticle motion per tick at full axis deflection (target-space units).
pub const AIM_RATE: i32 = 8;
/// Power gained per tick while charging.
pub const CHARGE_PER_TICK: i32 = 8;
/// Maximum power.
pub const POWER_MAX: i32 = 100;

/// Target-space bounds (the clamped goal-mouth rectangle).
pub const AIM_X_MIN: i32 = -100;
pub const AIM_X_MAX: i32 = 100;
pub const AIM_Y_MIN: i32 = 0;
pub const AIM_Y_MAX: i32 = 100;
/// The reticle starts at the center of the goal mouth.
pub const AIM_CENTER_X: i32 = 0;
pub const AIM_CENTER_Y: i32 = 50;

/// The aim target in normalized target space: `x ∈ [-100, 100]`,
/// `y ∈ [0, 100]`, clamped to the goal-mouth rectangle.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PenaltyAimState {
    pub target_x: i32,
    pub target_y: i32,
}

impl PenaltyAimState {
    /// The centered goal-mouth start position.
    pub const fn centered() -> Self {
        Self { target_x: AIM_CENTER_X, target_y: AIM_CENTER_Y }
    }

    /// Move by the (clamped) axes for one tick, then clamp into the rectangle.
    fn moved(self, aim_x_axis: i32, aim_y_axis: i32) -> Self {
        let dx = aim_x_axis * AIM_RATE / 100;
        let dy = aim_y_axis * AIM_RATE / 100;
        Self {
            target_x: (self.target_x + dx).clamp(AIM_X_MIN, AIM_X_MAX),
            target_y: (self.target_y + dy).clamp(AIM_Y_MIN, AIM_Y_MAX),
        }
    }
}

/// The shot-power meter value, `0..=100`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PenaltyPowerState {
    pub power: i32,
}

impl PenaltyPowerState {
    /// No charge.
    pub const fn zero() -> Self {
        Self { power: 0 }
    }

    /// One tick of charging (clamped at [`POWER_MAX`]).
    fn charged(self) -> Self {
        Self { power: (self.power + CHARGE_PER_TICK).min(POWER_MAX) }
    }
}

/// The interaction/shot state machine's discrete state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PenaltyShotFlightState {
    /// Reticle can move; power is `0`; ball at the penalty spot.
    Aiming,
    /// Reticle can still move; power increases while charge is held; ball at spot.
    Charging,
    /// Reticle and power are frozen after release; the ball still does not move.
    LockedPreview,
    /// The ball advances deterministically along its trajectory each tick.
    BallInFlight,
    /// The ball touched a goalie save volume; the contact frame is stored and
    /// the ball freezes. Resolved as a `Save` on the next tick.
    ContactDetected,
    /// The ball reached the goal plane untouched. Resolved as `Goal`/`Miss`/
    /// `Post` on the next tick.
    ArrivedAtGoalPlane,
    /// The shot is resolved: the final result + ball pose are frozen (the goalie
    /// may still finish its dive clip). Only reset leaves.
    Resolved,
}

/// The frozen, deterministic result of releasing a shot: the aim + power at
/// release. This is a **preview / locked intent**, not a `ShotResult` — it is
/// the stable descriptor a future Pass 5 will turn into a ball trajectory.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PenaltyShotPreview {
    pub target_x: i32,
    pub target_y: i32,
    pub power: i32,
    /// The local interaction tick the shot was locked on.
    pub release_tick: u32,
}

/// The full deterministic interaction/shot state: aim + power + flight state +
/// local tick + the optional locked preview + the optional live ball flight.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PenaltyInteractionState {
    pub aim: PenaltyAimState,
    pub power: PenaltyPowerState,
    pub state: PenaltyShotFlightState,
    pub tick: u32,
    /// Consecutive ticks the charge has been held (reset on release / reset /
    /// aiming). A **render-only** counter: it drives the time-based run-up in
    /// [`crate::soccer_penalty::penalty_kicker::kicker_frame`] so the kicker
    /// strides forward while the player holds, instead of the run-up being crushed
    /// into the power ramp. No scoring / flight / resolution logic reads it.
    pub charge_ticks: u32,
    pub preview: Option<PenaltyShotPreview>,
    /// Present once the ball has launched (`BallInFlight` / `ContactDetected` /
    /// `ArrivedAtGoalPlane`).
    pub flight: Option<PenaltyBallFlight>,
    /// The first goalie contact frame, if the ball touched a save volume.
    pub contact: Option<PenaltyGoalieContactFrame>,
    /// The goalie's deterministic dive animation (Pass 7).
    pub goalie: PenaltyGoalieAnimation,
    /// The frozen final result once the shot is `Resolved` (Pass 8).
    pub resolved: Option<PenaltyResolvedShotState>,
}

impl PenaltyInteractionState {
    /// The start state: centered aim, zero power, `Aiming`, tick `0`, ball at
    /// the penalty spot.
    pub const fn start() -> Self {
        Self {
            aim: PenaltyAimState::centered(),
            power: PenaltyPowerState::zero(),
            state: PenaltyShotFlightState::Aiming,
            tick: 0,
            charge_ticks: 0,
            preview: None,
            flight: None,
            contact: None,
            goalie: PenaltyGoalieAnimation::idle(),
            resolved: None,
        }
    }

    /// Advance one fixed tick with the given intent, returning the next state.
    /// Pure and total — the sole rule engine for aim / power / flight / goalie.
    ///
    /// Order per tick: step the shot (aim / power / ball), animate the goalie,
    /// then test the ball against the goalie's *current animated* volumes.
    pub fn advance(self, intent: PenaltyInputIntent) -> Self {
        let tick = self.tick + 1;

        if intent.reset_pressed {
            // Reset returns to a fresh Aiming state (preserving the clock), the
            // ball back at the spot, contact cleared, and the goalie idle.
            return Self {
                aim: PenaltyAimState::centered(),
                power: PenaltyPowerState::zero(),
                state: PenaltyShotFlightState::Aiming,
                tick,
                charge_ticks: 0,
                preview: None,
                flight: None,
                contact: None,
                goalie: PenaltyGoalieAnimation::idle(),
                resolved: None,
            };
        }

        // 1. Shot / aim / power / ball progression (no contact yet).
        let shot = self.step_shot(intent, tick);
        // 2. Goalie animation for the new shot state.
        let goalie = self.next_goalie(shot.state, shot.preview);
        // 3. Contact test against the goalie's animated volumes this tick.
        let (state, contact) = self.detect_contact(&shot, &goalie);

        Self { state, contact, goalie, ..shot }
    }

    /// Step the shot state (aim / power / flight) without contact detection.
    /// `ContactDetected` / `ArrivedAtGoalPlane` (reached last tick, so observable)
    /// resolve into `Resolved` here.
    fn step_shot(self, intent: PenaltyInputIntent, tick: u32) -> Self {
        match self.state {
            PenaltyShotFlightState::Aiming | PenaltyShotFlightState::Charging => {
                let aim = self.aim.moved(intent.aim_x_axis, intent.aim_y_axis);
                self.step_active(intent, aim, tick)
            }
            PenaltyShotFlightState::LockedPreview => self.launch(tick),
            PenaltyShotFlightState::BallInFlight => self.advance_flight(tick),
            PenaltyShotFlightState::ContactDetected => self.resolve_from_contact(tick),
            PenaltyShotFlightState::ArrivedAtGoalPlane => self.resolve_from_arrival(tick),
            PenaltyShotFlightState::Resolved => Self { tick, ..self },
        }
    }

    /// Resolve a goalie contact into a frozen `Save` result.
    fn resolve_from_contact(self, tick: u32) -> Self {
        let final_ball_position = self.ball_pose().position;
        let resolved = self.contact.map(|frame| PenaltyResolvedShotState {
            result: PenaltyShotResultResolver::from_contact(&frame),
            final_ball_position,
            crossing: None,
        });
        Self { state: PenaltyShotFlightState::Resolved, tick, resolved, ..self }
    }

    /// Resolve a goal-plane arrival into a frozen `Goal` / `Miss` / `Post`.
    fn resolve_from_arrival(self, tick: u32) -> Self {
        let resolved = self.flight.map(|flight| {
            let position = flight.pose().position;
            let preview = flight.descriptor.preview;
            let crossing = PenaltyGoalPlaneCrossing::at(
                flight.elapsed_ticks,
                position,
                preview.target_x,
                preview.target_y,
            );
            PenaltyResolvedShotState {
                result: PenaltyShotResultResolver::from_crossing(&crossing),
                final_ball_position: position,
                crossing: Some(crossing),
            }
        });
        Self { state: PenaltyShotFlightState::Resolved, tick, resolved, ..self }
    }

    /// The goalie's next animation: idle while aiming/charging, choose a lane on
    /// lock, then advance the dive clip while the shot is committed.
    fn next_goalie(
        self,
        shot_state: PenaltyShotFlightState,
        preview: Option<PenaltyShotPreview>,
    ) -> PenaltyGoalieAnimation {
        match shot_state {
            PenaltyShotFlightState::Aiming | PenaltyShotFlightState::Charging => {
                PenaltyGoalieAnimation::idle()
            }
            PenaltyShotFlightState::LockedPreview => preview
                .map(|p| PenaltyGoalieAnimation::locked(p.target_x, p.target_y))
                .unwrap_or_else(PenaltyGoalieAnimation::idle),
            PenaltyShotFlightState::BallInFlight
            | PenaltyShotFlightState::ContactDetected
            | PenaltyShotFlightState::ArrivedAtGoalPlane
            | PenaltyShotFlightState::Resolved => self.goalie.advanced(),
        }
    }

    /// Test the ball against the goalie's animated volumes (only while the ball
    /// is in flight). Contact freezes the ball into `ContactDetected`.
    fn detect_contact(
        &self,
        shot: &Self,
        goalie: &PenaltyGoalieAnimation,
    ) -> (PenaltyShotFlightState, Option<PenaltyGoalieContactFrame>) {
        let in_flight = matches!(shot.state, PenaltyShotFlightState::BallInFlight);
        let frame = in_flight
            .then_some(shot.flight)
            .flatten()
            .map(|f| PenaltyGoalieContactDetector::new(goalie.animated_volumes())
                .detect(f.pose().position, f.elapsed_ticks));
        let hit = frame.map(|fr| fr.contact.is_some()).unwrap_or(false);
        let state = if hit { PenaltyShotFlightState::ContactDetected } else { shot.state };
        let contact = hit.then_some(frame).flatten().or(self.contact);
        (state, contact)
    }

    /// The active-phase transition (Aiming/Charging).
    fn step_active(self, intent: PenaltyInputIntent, aim: PenaltyAimState, tick: u32) -> Self {
        // Release freezes the current power into a locked preview.
        let locked = Self {
            aim,
            power: self.power,
            state: PenaltyShotFlightState::LockedPreview,
            tick,
            charge_ticks: 0,
            preview: Some(PenaltyShotPreview {
                target_x: aim.target_x,
                target_y: aim.target_y,
                power: self.power.power,
                release_tick: tick,
            }),
            flight: None,
            contact: None,
            ..self
        };
        // Charge raises power and enters Charging; otherwise power holds and the
        // state is unchanged (Aiming stays Aiming, Charging holds). Each charging
        // tick advances the render-only run-up clock.
        let charged = Self {
            aim,
            power: self.power.charged(),
            state: PenaltyShotFlightState::Charging,
            tick,
            charge_ticks: self.charge_ticks + 1,
            preview: None,
            flight: None,
            contact: None,
            ..self
        };
        let held = Self { aim, state: self.state, tick, preview: None, flight: None, contact: None, ..self };

        // Release wins over charge; charge wins over hold.
        let charge_or_hold = [held, charged][intent.charge_pressed as usize];
        [charge_or_hold, locked][intent.release_pressed as usize]
    }

    /// Launch the ball from the locked preview (no-op-ish if somehow unset).
    fn launch(self, tick: u32) -> Self {
        let launched = self.preview.map(|preview| Self {
            state: PenaltyShotFlightState::BallInFlight,
            tick,
            flight: Some(PenaltyBallFlight::launch(preview)),
            ..self
        });
        launched.unwrap_or(Self { tick, ..self })
    }

    /// Advance the live flight one tick, arriving at the goal plane when done
    /// (contact detection happens separately, in `detect_contact`).
    fn advance_flight(self, tick: u32) -> Self {
        let flight = self.flight.map(|f| f.advanced());
        let arrived = flight.map(|f| f.arrived()).unwrap_or(false);
        let state = if arrived { PenaltyShotFlightState::ArrivedAtGoalPlane } else { PenaltyShotFlightState::BallInFlight };
        Self { state, tick, flight, ..self }
    }

    /// The ball's current pose: the live flight pose, or the resting pose at the
    /// penalty spot when not in flight.
    pub fn ball_pose(&self) -> PenaltyBallPose {
        self.flight.map(|f| f.pose()).unwrap_or_else(resting_pose)
    }

    /// A coarse, ball-focused view of the current situation. `ContactDetected`
    /// reads as in-flight (the ball is frozen mid-flight at the contact point).
    pub fn ball_state(&self) -> PenaltyBallState {
        match self.state {
            PenaltyShotFlightState::BallInFlight
            | PenaltyShotFlightState::ContactDetected
            | PenaltyShotFlightState::Resolved => PenaltyBallState::InFlight,
            PenaltyShotFlightState::ArrivedAtGoalPlane => PenaltyBallState::ArrivedAtGoalPlane,
            _ => PenaltyBallState::AtPenaltySpot,
        }
    }

    /// Fold a whole intent sequence from the start state — the deterministic
    /// driver tests and future host loops use.
    pub fn run(intents: &[PenaltyInputIntent]) -> Self {
        intents.iter().fold(Self::start(), |state, &intent| state.advance(intent))
    }
}

impl Default for PenaltyInteractionState {
    fn default() -> Self {
        Self::start()
    }
}
