//! The command vocabulary the simulation accepts, and the one place a
//! command is turned into a state change.
//!
//! Split out of [`crate::state`] so the orchestrator file stays a readable
//! description of ONE tick, rather than a tick plus an input dictionary.

use crate::identity::PlayerId;
use crate::state::SimState;

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
    /// Order the quarterback to throw to a NAMED receiver. This is how a read
    /// is committed: the player chose a target by number, so the cone's
    /// nearest-the-centre-line pick must not override them.
    ///
    /// Every pass this commands is on the money — there is no power to get
    /// wrong. The decision the game asks for is *which receiver, and when*, and
    /// a throw that missed for a third reason the player never named would be
    /// answering a question nobody asked.
    ThrowTo(PlayerId),
    /// The quarterback abandons the pocket and runs. Distinct from simply
    /// steering him: it tells the defense he is a runner immediately, instead
    /// of waiting for the scramble detector to notice.
    Scramble,
    /// Hand the ball to `PlayerId` — the exchange. Refused unless the field
    /// agrees (see [`SimState::hand_off`]), so ordering one is a request, never
    /// a possession transfer.
    HandOff(PlayerId),
    /// The running back's move for this tick. The whole of the player's live
    /// control: there is no movement command, because the run is the AI's.
    Runback(crate::runback::RunbackMove),
    /// Reset to formation without starting (diagnostic R).
    ResetPlay,
}

impl SimState {
    pub(crate) fn apply_command(&mut self, command: SimCommand) {
        match command {
            SimCommand::BeginPlay => self.reset_to_formation(true),
            SimCommand::Snap => self.snap(),
            SimCommand::ThrowNow => self.throw_commanded = true,
            SimCommand::ThrowTo(target) => {
                self.throw_commanded = true;
                self.declared_target = Some(target);
            }
            SimCommand::Scramble => self.qb_scrambling = true,
            SimCommand::HandOff(back) => self.hand_off(back),
            // Latched, not applied: the runback stage is the authority on
            // whether the move is legal this tick, so a command that arrives
            // during a cooldown is dropped there rather than half-honoured here.
            SimCommand::Runback(wanted) => self.runback.pending = Some(wanted),
            SimCommand::ResetPlay => self.reset_to_formation(false),
        }
    }

}
