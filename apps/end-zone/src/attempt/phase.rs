//! The attempt's explicit state — one enum, never a pile of booleans.
//!
//! Simulation progress (`PlayPhase`: PreSnap/Live/Ended) and *attempt* progress
//! are deliberately different things: the simulation only knows whether a play
//! is running, while the attempt knows whether the player is calling it,
//! watching it come to them, or carrying it. Everything the loop branches on
//! lives here, so a reader can see the whole game in one enum.
//!
//! There is exactly one moment that matters in it — [`AttemptPhase::Carrying`],
//! the phase in which the player has a body. Everything before it is the play
//! being set up *for* them, and the shape of this enum is the promise that the
//! handoff is a real beat with a real duration rather than a flag flipping.

/// The attempt lifecycle. Every field a state needs is carried *in* the state,
/// so there is no way to be mid-exchange without knowing when it started.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AttemptPhase {
    /// At the line with the play card up, waiting for the player to call one.
    /// **There is no clock.** The snap belongs to the player, so this holds for
    /// as long as it takes — an attempt never runs a play nobody chose.
    PlayCall,
    /// The call is in and the offense is sprinting into its new alignment. The
    /// ball snaps the moment every offensive player is on his spot, so the
    /// shift is not a wait — it IS the snap count. `stalled_at` is a stall
    /// guard, not a deadline.
    Shifting { stalled_at: u64 },
    /// Snapped. The quarterback is opening away from the line and the back is
    /// coming to meet him. The player has no body yet and their moves are
    /// stale — the play is still being handed to them.
    Mesh { snapped_at: u64 },
    /// **The exchange.** The ball is visibly travelling from one pair of hands
    /// to the other. Its own phase rather than a boolean because it is the beat
    /// the whole game hands over on, and the player has to be able to see it
    /// happen before they are asked to do anything with it.
    Exchange,
    /// The back has the ball and the player has the back. The only phase in
    /// which a juke, a charge or a leap means anything.
    Carrying,
    /// The simulation ended the play; the outcome is being measured.
    Resolving,
    /// Holding the result card until `until`.
    Result { until: u64 },
    /// Tearing down and rebuilding for the next attempt.
    Resetting,
}

impl AttemptPhase {
    /// Time dilation this phase runs at.
    ///
    /// Uniformly [`super::RUN_TIME_SCALE`] — the run game never slows down, and
    /// deliberately: reading a closing defender at full speed and answering him
    /// in time IS the game, and a game that stopped to ask would be answering
    /// the question for you. The fractional tick-credit stepping in
    /// [`crate::app::EndZoneApp::advance`] and the render interpolation in
    /// [`crate::presentation::interpolate`] are both still keyed on
    /// `time_scale < 1.0`, so a phase that wanted to dilate could.
    pub fn time_scale(self) -> f32 {
        super::RUN_TIME_SCALE
    }

    /// Whether the number row is calling plays right now.
    pub fn accepts_call(self) -> bool {
        matches!(self, AttemptPhase::PlayCall)
    }

    /// Whether a runback move issued this tick should reach the simulation.
    ///
    /// Only while carrying. A juke pressed during the mesh is not banked for
    /// later — it is stale, exactly like a throw pressed before the snap used
    /// to be, because a move is a response to a defender and a response saved up
    /// for four ticks' time is a response to nothing.
    pub fn controllable(self) -> bool {
        matches!(self, AttemptPhase::Carrying)
    }

    /// Whether the move controls should be on screen. From the exchange onward:
    /// a control you are about to be able to use is one you should already be
    /// able to see.
    pub fn shows_moves(self) -> bool {
        matches!(self, AttemptPhase::Exchange | AttemptPhase::Carrying)
    }

    /// Whether the play underneath is running.
    pub fn is_live(self) -> bool {
        matches!(
            self,
            AttemptPhase::Mesh { .. } | AttemptPhase::Exchange | AttemptPhase::Carrying
        )
    }

    /// A short debug/HUD label.
    pub fn label(self) -> &'static str {
        match self {
            AttemptPhase::PlayCall => "play-call",
            AttemptPhase::Shifting { .. } => "shifting",
            AttemptPhase::Mesh { .. } => "mesh",
            AttemptPhase::Exchange => "exchange",
            AttemptPhase::Carrying => "carrying",
            AttemptPhase::Resolving => "resolving",
            AttemptPhase::Result { .. } => "result",
            AttemptPhase::Resetting => "resetting",
        }
    }
}
