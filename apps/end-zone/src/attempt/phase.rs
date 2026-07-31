//! The attempt's explicit state — one enum, never a pile of booleans.
//!
//! Simulation progress (`PlayPhase`: PreSnap/Live/Ended) and *attempt* progress
//! are deliberately different things: the simulation only knows whether a play
//! is running, while the attempt knows whether the player is watching, being
//! asked, or living with the answer. Everything the loop branches on lives
//! here, so a reader can see the whole prototype in one enum.

/// Why a decision window opened. Shown to the player as the window's headline,
/// because *why you are being asked* is half the read.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WindowTrigger {
    /// A receiver came open — the moment the read is worth making.
    ReadOpen,
    /// The pocket is about to break. Decide now, on worse information.
    Pressure,
    /// The develop deadline elapsed. Guarantees every attempt offers a window.
    Deadline,
}

impl WindowTrigger {
    /// The headline shown above the choices.
    pub fn label(self) -> &'static str {
        match self {
            WindowTrigger::ReadOpen => "READ IT",
            WindowTrigger::Pressure => "PRESSURE",
            WindowTrigger::Deadline => "THROW IT",
        }
    }
}

/// What the player chose during a decision window. Exactly one of three things —
/// there is no "do nothing" choice, because doing nothing is letting the window
/// close, which the loop models as its own outcome.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlayerChoice {
    /// Throw to read `0..3` (read 0 is the short one). One press, one throw:
    /// the pass is on the money, so the decision is purely *which receiver and
    /// when* — never *how hard*. The loop issues the throw itself.
    Throw(usize),
    /// Abandon the pocket — the player takes direct control of the quarterback.
    Scramble,
}

impl PlayerChoice {
    /// The read this choice throws to, if it is a throw.
    pub fn read(self) -> Option<usize> {
        match self {
            PlayerChoice::Throw(read) => Some(read),
            PlayerChoice::Scramble => None,
        }
    }
}

/// The attempt lifecycle. Every field a state needs is carried *in* the state,
/// so there is no way to be in `DecisionWindow` without knowing when it closes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AttemptPhase {
    /// At the line with the play card up, waiting for the player to call one.
    /// **There is no clock.** The snap belongs to the player, so this holds for
    /// as long as it takes — an attempt never starts on a play nobody chose.
    PlayCall,
    /// The call is in and the offense is sprinting into its new alignment. The
    /// ball snaps the moment every offensive player is on his spot, so the
    /// shift is not a wait — it IS the snap count, and it is the feedback that
    /// the call took. `stalled_at` is a stall guard, not a deadline: it exists
    /// only so a player who somehow cannot reach his spot can never hang the
    /// attempt.
    Shifting { stalled_at: u64 },
    /// The play runs itself: drop-back, routes, protection, rush. The player
    /// watches and reads; the window gate decides when to interrupt.
    Developing,
    /// Slow motion. The player has until `closes_at` to pick one of four things.
    DecisionWindow {
        opened_at: u64,
        closes_at: u64,
        trigger: WindowTrigger,
    },
    /// The pass is away — flight, catch or break-up, then the run after it.
    PassInFlight { read: usize },
    /// The quarterback is running and the player is steering him.
    Scrambling,
    /// The simulation ended the play; the outcome is being measured.
    Resolving,
    /// Holding the result card until `until`.
    Result { until: u64 },
    /// Tearing down and rebuilding for the next attempt.
    Resetting,
}

impl AttemptPhase {
    /// Time dilation this phase runs at. Only the decision window slows down —
    /// everything else is full speed, which is what makes the slow motion read
    /// as an intervention rather than a setting.
    pub fn time_scale(self) -> f32 {
        match self {
            AttemptPhase::DecisionWindow { .. } => super::DECISION_TIME_SCALE,
            _ => 1.0,
        }
    }

    /// Whether the player may choose a read right now.
    ///
    /// True from the snap onward, NOT just inside a window. The quarterback can
    /// throw whenever he likes once he has the ball, so an anticipatory throw —
    /// releasing before the receiver breaks, at full speed — is a real option
    /// and a rewarded one. The decision window is not permission to act; it is
    /// the game slowing down to make sure the player does not miss the moment.
    ///
    /// A press before the snap is still stale and is dropped: there is no ball.
    pub fn accepts_choice(self) -> bool {
        matches!(
            self,
            AttemptPhase::Developing | AttemptPhase::DecisionWindow { .. }
        )
    }

    /// Whether the three numbered reads should be on screen — their field
    /// markers and their prompt.
    ///
    /// They appear the moment a play is CALLED, not before: during the call
    /// itself the three numbers mean plays, and showing receiver numerals for a
    /// concept nobody has chosen would be labelling routes that are about to
    /// change. From the shift onward they stay up through the whole live play,
    /// because a control you can use is a control you should be able to see.
    pub fn shows_reads(self) -> bool {
        matches!(
            self,
            AttemptPhase::Shifting { .. }
                | AttemptPhase::Developing
                | AttemptPhase::DecisionWindow { .. }
        )
    }

    /// Whether the player's stick steers the quarterback. Only during a
    /// scramble: while the play develops the simulation owns every player, and
    /// that is the point of the prototype.
    pub fn steerable(self) -> bool {
        matches!(
            self,
            AttemptPhase::Scrambling | AttemptPhase::PassInFlight { .. }
        )
    }

    /// Whether the decision-window presentation (rings, prompt, slow motion)
    /// should be showing.
    pub fn in_window(self) -> bool {
        matches!(self, AttemptPhase::DecisionWindow { .. })
    }

    /// A short debug/HUD label.
    pub fn label(self) -> &'static str {
        match self {
            AttemptPhase::PlayCall => "play-call",
            AttemptPhase::Shifting { .. } => "shifting",
            AttemptPhase::Developing => "developing",
            AttemptPhase::DecisionWindow { .. } => "decision",
            AttemptPhase::PassInFlight { .. } => "in-flight",
            AttemptPhase::Scrambling => "scrambling",
            AttemptPhase::Resolving => "resolving",
            AttemptPhase::Result { .. } => "result",
            AttemptPhase::Resetting => "resetting",
        }
    }
}

/// How long the `n`-th window (0-based) stays open. Each successive look is
/// shorter: the longer the player waits for a better read, the less time they
/// get to take it.
pub fn window_length(windows_used: u32) -> u64 {
    super::WINDOW_TICKS
        .saturating_sub(super::WINDOW_DECAY_TICKS * u64::from(windows_used))
        .max(super::WINDOW_MIN_TICKS)
}
