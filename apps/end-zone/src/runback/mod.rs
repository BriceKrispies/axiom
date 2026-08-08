//! **The running back's game.** Four verbs, one of which is not a verb at all.
//!
//! Once the back has the ball he runs downfield on his own — that is the AI's
//! `Carry` intent, unchanged, and it is the reason this module owns no steering
//! and no free movement. What the player controls is not *where* the back runs
//! but *what he does about the man in front of him*:
//!
//! | Move | Answers | Costs |
//! |---|---|---|
//! | juke left / right | a defender committed to your current line | a little forward speed, and a beat you cannot move again |
//! | shoulder charge | a defender you have the momentum to go *through* | everything, if you were wrong |
//! | leap | a defender low enough to go *over* | most of a second airborne, then three seconds of waiting |
//!
//! Three moves, three different geometries, no dominant answer — which is the
//! design, and which is why each one reads different numbers off the same
//! contact (see [`charge`] for the one that reads the most).
//!
//! ## Where this sits
//!
//! It is **simulation**, not presentation: it writes authoritative position and
//! velocity, and it is a pure function of `(tick, command stream)` like
//! everything else under `SimState`. It sits alongside [`crate::player::contact`]
//! in the tick order and for the same reason — both own the motion of a player
//! in a state the ordinary controller has no opinion about. The controller still
//! owns the *run*; this owns the *move*.
//!
//! Nothing here knows about a keyboard or a touchscreen. [`RunbackMove`] is the
//! whole vocabulary, and both input surfaces produce it (see
//! [`crate::controls`]).

pub mod charge;
pub mod evade;
pub mod stage;

pub use charge::ChargeResolution;
pub use evade::{HurdleWatch, ThreatSnapshot};

use crate::events::RunbackMoveCode;
use crate::identity::PlayerId;

/// The four things the player can ask the running back to do. One enum, shared
/// by the keyboard, the swipe recogniser, the agent, and the simulation — so
/// there is exactly one gameplay system underneath every input surface.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RunbackMove {
    /// Plant and cut to the offense's left.
    JukeLeft,
    /// Plant and cut to the offense's right.
    JukeRight,
    /// Lower the shoulder and run through the next man.
    Shoulder,
    /// Leap.
    Jump,
}

impl RunbackMove {
    /// The event-stream code for this move.
    pub fn code(self) -> RunbackMoveCode {
        match self {
            RunbackMove::JukeLeft => RunbackMoveCode::JukeLeft,
            RunbackMove::JukeRight => RunbackMoveCode::JukeRight,
            RunbackMove::Shoulder => RunbackMoveCode::Shoulder,
            RunbackMove::Jump => RunbackMoveCode::Jump,
        }
    }

    /// The sign of a juke along the offense's right hand (`0` for the moves that
    /// are not jukes).
    pub fn juke_sign(self) -> f32 {
        match self {
            RunbackMove::JukeLeft => -1.0,
            RunbackMove::JukeRight => 1.0,
            _ => 0.0,
        }
    }

    /// A short label (HUD, agent trace, debug overlay).
    pub fn label(self) -> &'static str {
        self.code().label()
    }
}

/// The move the back is currently executing, and when it ends.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ActiveMove {
    pub kind: RunbackMove,
    /// The tick it began.
    pub started: u64,
    /// The tick it stops being active.
    pub ends: u64,
}

/// The running back's authoritative move state — one field on `SimState`.
///
/// Everything the moves need to be replayable lives here and nowhere else: no
/// wall clock, no hidden statics, and every timer is an absolute simulation
/// tick rather than a countdown, so a state read at tick *N* means the same
/// thing however the run got there.
#[derive(Debug, Clone, PartialEq)]
pub struct RunbackSim {
    /// The offense's running back for the installed play — the player-controlled
    /// slot. `None` on a play that fields no back (the ambient pass showcase).
    pub back: Option<PlayerId>,
    /// A move commanded this tick, awaiting the stage. Consumed once.
    pub(crate) pending: Option<RunbackMove>,
    /// The move in progress.
    pub active: Option<ActiveMove>,
    /// The tick at which any move may next begin (move recovery).
    pub ready_at: u64,
    /// The tick at which a **jump** may next begin. Separate from `ready_at`
    /// because the leap's cost is its own three-second wait, not the shared
    /// recovery every move pays.
    pub jump_ready_at: u64,
    /// Whether the back is airborne under his own leap.
    pub airborne: bool,
    /// Vertical velocity of the leap, yd/s.
    pub(crate) vertical: f32,
    /// The lateral unit direction the live juke is carrying him.
    pub(crate) juke_dir: axiom::prelude::Vec3,
    /// Defenders the live juke was aimed at, awaiting a verdict.
    pub(crate) threats: Vec<ThreatSnapshot>,
    /// Defenders that have passed beneath the live leap.
    pub(crate) hurdles: Vec<HurdleWatch>,
    /// The most recent charge and every term that decided it — the answer to
    /// "why did that resolve the way it did", for a test, the overlay, and the
    /// agent.
    pub last_charge: Option<ChargeResolution>,
    /// This play's confirmed successes.
    pub dodges: u32,
    pub hurdled: u32,
    pub broken: u32,
    /// The most recent confirmed success and the tick it landed, for the HUD's
    /// one-line feedback.
    pub last_success: Option<(RunbackMoveCode, u64)>,
}

impl RunbackSim {
    /// A back with no move in progress and a jump ready immediately.
    pub fn new() -> Self {
        RunbackSim {
            back: None,
            pending: None,
            active: None,
            ready_at: 0,
            jump_ready_at: 0,
            airborne: false,
            vertical: 0.0,
            juke_dir: axiom::prelude::Vec3::ZERO,
            threats: Vec::new(),
            hurdles: Vec::new(),
            last_charge: None,
            dodges: 0,
            hurdled: 0,
            broken: 0,
            last_success: None,
        }
    }

    /// Clear everything a play owns, keeping nothing across the whistle.
    pub fn reset_play(&mut self, back: Option<PlayerId>) {
        let last_charge = None;
        *self = RunbackSim {
            back,
            last_charge,
            ..RunbackSim::new()
        };
    }

    /// Ticks of jump cooldown remaining at `tick` (`0` when it is ready).
    pub fn jump_cooldown_left(&self, tick: u64) -> u64 {
        self.jump_ready_at.saturating_sub(tick)
    }

    /// Whether a jump may begin at `tick`: nothing else in progress, not already
    /// in the air, and the cooldown elapsed.
    pub fn jump_available(&self, tick: u64) -> bool {
        self.active.is_none() && !self.airborne && tick >= self.jump_ready_at
    }

    /// Whether any move may begin at `tick`.
    pub fn move_available(&self, tick: u64) -> bool {
        self.active.is_none() && tick >= self.ready_at
    }

    /// The move in progress, if any.
    pub fn active_move(&self) -> Option<RunbackMove> {
        self.active.map(|a| a.kind)
    }
}

impl Default for RunbackSim {
    fn default() -> Self {
        RunbackSim::new()
    }
}

/// The read-only snapshot of the back's move state — what the HUD, the camera,
/// the presentation snapshot and the agent all read, so none of them reaches
/// into [`RunbackSim`] itself.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RunbackStatus {
    pub back: Option<PlayerId>,
    pub active: Option<RunbackMove>,
    pub airborne: bool,
    /// Feet height above the turf, yd (`0` on the ground).
    pub height: f32,
    pub jump_available: bool,
    pub jump_cooldown_left: u64,
    pub dodges: u32,
    pub hurdled: u32,
    pub broken: u32,
    pub last_success: Option<(RunbackMoveCode, u64)>,
}
