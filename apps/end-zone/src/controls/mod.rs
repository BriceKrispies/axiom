//! The game's fixed input map: which key or gesture means what, and how one
//! rendered frame of raw input becomes typed commands. Documented for players in
//! `CONTROLS.md`.
//!
//! **One gameplay vocabulary, two surfaces.** Every input the player has resolves
//! to a [`crate::runback::RunbackMove`] or a play call, and nothing else — there
//! is no movement axis, because the running back runs by himself. So `W`/`A`/`S`/`D`
//! and the four swipes are not two control schemes; they are two spellings of the
//! same four verbs, and they meet in [`DiagnosticCommand::Move`] before anything
//! gameplay-shaped sees them.
//!
//! | Verb | Key | Swipe |
//! |---|---|---|
//! | juke left | `A` / `←` | ◀ |
//! | juke right | `D` / `→` | ▶ |
//! | shoulder charge | `S` / `↓` | ▼ |
//! | leap | `W` / `↑` | ▲ |
//!
//! The number row belongs to the play call: `1`/`2`/`3` pick the concept before
//! the snap and mean nothing after it. Camera diagnostics live on F2–F6 for
//! exactly the reason they always did — nothing is more confusing than a
//! gameplay key that also moves the camera.
//!
//! Presses are *latched* rather than consumed immediately. Input is sampled
//! every rendered frame but consumed once per simulation tick, and a frame is
//! not a tick: a press landing on a frame that shares its tick with others must
//! still survive to it. The latch is what stops those inputs being eaten.

pub mod swipe;

use axiom_input::{ActionId, DeviceFrame, InputState, KeyToken};
use axiom_kernel::Tick;

pub use swipe::{SwipeRecognizer, SwipeSample};

use crate::runback::RunbackMove;

/// Diagnostic + gameplay input commands — the vocabulary this map emits.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DiagnosticCommand {
    /// Space: start the play, or restart it after completion (ambient only).
    StartPlay,
    /// R: reset all showcase state to formation (idle until started).
    ResetAll,
    /// The contextual action button (touch A / Enter): snaps the ambient play
    /// and orders the cone-aimed throw while the ambient quarterback holds it.
    PrimaryAction,
    /// `1`/`2`/`3` pressed — the play call. ONE command for the whole number
    /// row, because the row means one thing: which concept to run.
    SelectPlay(usize),
    /// **The running back's move.** The whole of the player's live control.
    Move(RunbackMove),
    /// F2–F6: force a camera mode; F6 returns to automatic.
    ForceFormationCamera,
    ForceQuarterbackCamera,
    ForceFlightCamera,
    ForceCarrierCamera,
    AutomaticCamera,
    /// F1: toggle the diagnostic overlays.
    ToggleDebug,
}

/// Gameplay actions.
const ACTION_START: ActionId = ActionId::new(1);
const ACTION_RESET: ActionId = ActionId::new(2);
const ACTION_PRIMARY: ActionId = ActionId::new(9);
const ACTION_JUMP: ActionId = ActionId::new(10);
const ACTION_SHOULDER: ActionId = ActionId::new(11);
const ACTION_JUKE_LEFT: ActionId = ActionId::new(12);
const ACTION_JUKE_RIGHT: ActionId = ActionId::new(13);
const ACTION_PLAY_ONE: ActionId = ActionId::new(14);
const ACTION_PLAY_TWO: ActionId = ActionId::new(15);
const ACTION_PLAY_THREE: ActionId = ActionId::new(16);
/// Diagnostic actions.
const ACTION_CAM_FORMATION: ActionId = ActionId::new(3);
const ACTION_CAM_QB: ActionId = ActionId::new(4);
const ACTION_CAM_FLIGHT: ActionId = ActionId::new(5);
const ACTION_CAM_CARRIER: ActionId = ActionId::new(6);
const ACTION_CAM_AUTO: ActionId = ActionId::new(7);
const ACTION_DEBUG: ActionId = ActionId::new(8);

/// How many commands may wait for the next simulation tick.
const COMMAND_CAP: usize = 8;

/// One frame of pointer/gamepad input from the platform edge, already debounced
/// to single-frame edges.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct TouchInput {
    pub primary: bool,
    pub reset: bool,
    /// A play row TAPPED this frame, `0..3` — a one-shot edge, matching the
    /// keyboard's press.
    pub play: Option<usize>,
    /// A swipe COMPLETED this frame, already recognised by
    /// [`swipe::SwipeRecognizer`] on the deterministic side of the boundary.
    pub swipe: Option<RunbackMove>,
}

/// The deterministic input sampler plus the latch of commands awaiting a tick.
#[derive(Debug)]
pub struct GameInput {
    state: InputState,
    /// Input sample counter — one per rendered frame, distinct from both the
    /// engine frame index and the simulation tick.
    sample_n: u64,
    pending: Vec<DiagnosticCommand>,
}

impl Default for GameInput {
    fn default() -> Self {
        GameInput::new()
    }
}

impl GameInput {
    /// The bound input map.
    pub fn new() -> Self {
        let mut state = InputState::new();
        state.bind_action(ACTION_START, &[KeyToken::new("Space")]);
        state.bind_action(ACTION_RESET, &[KeyToken::new("KeyR")]);
        state.bind_action(ACTION_PLAY_ONE, &[KeyToken::new("Digit1")]);
        state.bind_action(ACTION_PLAY_TWO, &[KeyToken::new("Digit2")]);
        state.bind_action(ACTION_PLAY_THREE, &[KeyToken::new("Digit3")]);
        state.bind_action(ACTION_CAM_FORMATION, &[KeyToken::new("F2")]);
        state.bind_action(ACTION_CAM_QB, &[KeyToken::new("F3")]);
        state.bind_action(ACTION_CAM_FLIGHT, &[KeyToken::new("F4")]);
        state.bind_action(ACTION_CAM_CARRIER, &[KeyToken::new("F5")]);
        state.bind_action(ACTION_CAM_AUTO, &[KeyToken::new("F6")]);
        state.bind_action(ACTION_DEBUG, &[KeyToken::new("F1")]);
        state.bind_action(ACTION_PRIMARY, &[KeyToken::new("Enter")]);
        // The four moves. Arrow keys mirror WASD so the left hand and the right
        // hand play the same game.
        state.bind_action(ACTION_JUMP, &[KeyToken::new("KeyW"), KeyToken::new("ArrowUp")]);
        state.bind_action(
            ACTION_SHOULDER,
            &[KeyToken::new("KeyS"), KeyToken::new("ArrowDown")],
        );
        state.bind_action(
            ACTION_JUKE_LEFT,
            &[KeyToken::new("KeyA"), KeyToken::new("ArrowLeft")],
        );
        state.bind_action(
            ACTION_JUKE_RIGHT,
            &[KeyToken::new("KeyD"), KeyToken::new("ArrowRight")],
        );
        GameInput {
            state,
            sample_n: 0,
            pending: Vec::new(),
        }
    }

    /// Sample one rendered frame and latch its press edges.
    ///
    /// Every move is a **press**, never a hold: a juke is an event, and holding
    /// `A` down is not a request to keep juking — it is a finger resting on a
    /// key. That is the same rule the swipe recogniser enforces on the other
    /// surface, which is why the two feel identical.
    pub fn sample(
        &mut self,
        size: axiom::prelude::Vec2,
        keys_down: &[KeyToken],
        touch: TouchInput,
    ) {
        let frame = DeviceFrame::new(size, keys_down, &[]);
        self.state.sample(Tick::new(self.sample_n), &frame);
        self.sample_n += 1;

        let plays = [ACTION_PLAY_ONE, ACTION_PLAY_TWO, ACTION_PLAY_THREE];
        let called = plays
            .iter()
            .position(|action| self.state.pressed(*action))
            .or(touch.play.map(|play| play.min(2)));
        if let Some(play) = called {
            self.latch(DiagnosticCommand::SelectPlay(play));
        }

        let moves: [(ActionId, RunbackMove); 4] = [
            (ACTION_JUKE_LEFT, RunbackMove::JukeLeft),
            (ACTION_JUKE_RIGHT, RunbackMove::JukeRight),
            (ACTION_SHOULDER, RunbackMove::Shoulder),
            (ACTION_JUMP, RunbackMove::Jump),
        ];
        for (action, wanted) in moves {
            if self.state.pressed(action) {
                self.latch(DiagnosticCommand::Move(wanted));
            }
        }
        if let Some(wanted) = touch.swipe {
            self.latch(DiagnosticCommand::Move(wanted));
        }

        let pressed: [(ActionId, DiagnosticCommand); 9] = [
            (ACTION_START, DiagnosticCommand::StartPlay),
            (ACTION_RESET, DiagnosticCommand::ResetAll),
            (
                ACTION_CAM_FORMATION,
                DiagnosticCommand::ForceFormationCamera,
            ),
            (ACTION_CAM_QB, DiagnosticCommand::ForceQuarterbackCamera),
            (ACTION_CAM_FLIGHT, DiagnosticCommand::ForceFlightCamera),
            (ACTION_CAM_CARRIER, DiagnosticCommand::ForceCarrierCamera),
            (ACTION_CAM_AUTO, DiagnosticCommand::AutomaticCamera),
            (ACTION_DEBUG, DiagnosticCommand::ToggleDebug),
            (ACTION_PRIMARY, DiagnosticCommand::PrimaryAction),
        ];
        for (action, command) in pressed {
            if self.state.pressed(action) {
                self.latch(command);
            }
        }
        if touch.primary {
            self.latch(DiagnosticCommand::PrimaryAction);
        }
        if touch.reset {
            self.latch(DiagnosticCommand::ResetAll);
        }
    }

    /// Take the latched commands for one simulation tick.
    pub fn drain(&mut self) -> Vec<DiagnosticCommand> {
        core::mem::take(&mut self.pending)
    }

    /// Drop every latched command (a run swap must not inherit a press).
    pub fn clear(&mut self) {
        self.pending.clear();
    }

    /// Queue one command for the next simulation tick (deduplicated, bounded).
    fn latch(&mut self, command: DiagnosticCommand) {
        if !self.pending.contains(&command) && self.pending.len() < COMMAND_CAP {
            self.pending.push(command);
        }
    }
}
