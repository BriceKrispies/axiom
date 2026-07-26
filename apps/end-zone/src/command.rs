//! The command vocabulary the simulation accepts, and the one place a
//! command is turned into a state change.
//!
//! Split out of [`crate::state`] so the orchestrator file stays a readable
//! description of ONE tick, rather than a tick plus an input dictionary.

use crate::identity::PlayerId;
use crate::state::{SimState, CHARGE_MAX_TICKS};

/// Sentinel power meaning "solve it": the simulation picks enough speed to
/// reach the read, rather than a power the player named.
pub const AUTO_POWER: f32 = -1.0;

/// Commands the simulation accepts (issued by the run loop and the diagnostic
/// input). The quarterback never throws or runs on his own — every one of
/// these originates in a player decision or a scripted harness standing in for
/// one.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SimCommand {
    /// (Re)set the play to formation and mark it started.
    BeginPlay,
    /// Snap the ball.
    Snap,
    /// Order the quarterback to throw, letting the throwing cone pick the
    /// receiver (the stick aims the pass).
    ThrowNow,
    /// Order the quarterback to throw to a NAMED receiver at full power. This
    /// is how the decision window commits a read: the player chose a target by
    /// number, so the cone's nearest-the-centre-line pick must not override
    /// them.
    ThrowTo(PlayerId),
    /// Hold the wind-up on a named receiver. Issued every tick the read is
    /// held;  is how much charge that tick is worth.
    ///
    /// The gain exists because a tick is not a fixed amount of REAL time: in a
    /// decision windows dilation one tick covers ~8 rendered frames, so a
    /// flat one-per-tick wind-up visibly crawled during the window and then
    /// leapt when full speed returned. The run loop scales the gain by the
    /// inverse time scale, which keeps the meter filling at a constant rate on
    /// the players clock while the wind-up stays a deterministic function of
    /// simulation ticks.
    ChargeThrow { target: PlayerId, gain: u32 },
    /// Let the wind-up go, throwing at whatever charge was accumulated.
    ReleaseThrow,
    /// The quarterback abandons the pocket and runs. Distinct from simply
    /// steering him: it tells the defense he is a runner immediately, instead
    /// of waiting for the scramble detector to notice.
    Scramble,
    /// Reset to formation without starting (diagnostic R).
    ResetPlay,
}

impl SimState {
    pub(crate) fn apply_command(&mut self, command: SimCommand) {
        match command {
            SimCommand::BeginPlay => self.reset_to_formation(true),
            SimCommand::Snap => self.snap(),
            SimCommand::ThrowNow => {
                self.throw_commanded = true;
                self.throw_power = AUTO_POWER;
            }
            SimCommand::ThrowTo(target) => {
                self.throw_commanded = true;
                self.declared_target = Some(target);
                self.throw_power = AUTO_POWER;
            }
            // Winding up: one tick of charge per tick the read is held. Aiming
            // at a different read restarts the wind-up rather than inheriting
            // the previous one's charge.
            SimCommand::ChargeThrow { target, gain } => {
                let switched = self.charge_target != Some(target);
                self.charge_target = Some(target);
                self.charge_ticks = match switched {
                    true => 0,
                    false => (self.charge_ticks + gain.max(1)).min(CHARGE_MAX_TICKS),
                };
            }
            SimCommand::ReleaseThrow => {
                if let Some(target) = self.charge_target.take() {
                    self.throw_power = self.charge_ratio();
                    self.charge_ticks = 0;
                    self.throw_commanded = true;
                    self.declared_target = Some(target);
                }
            }
            SimCommand::Scramble => self.qb_scrambling = true,
            SimCommand::ResetPlay => self.reset_to_formation(false),
        }
    }

}
