//! The game's fixed input map: which key or touch control means what, and how
//! one rendered frame of raw input becomes typed commands plus a movement
//! stick. Documented for players in `CONTROLS.md`.
//!
//! The number row belongs to **gameplay**: during a decision window `1`/`2`/`3`
//! ARE the three reads, and `Space` is the scramble. Camera diagnostics were
//! moved onto F2–F6 for exactly that reason — nothing is more confusing than a
//! read key that also moves the camera.
//!
//! Presses are *latched* rather than consumed immediately. Input is sampled
//! every rendered frame, but simulation ticks are spent according to the run's
//! time scale, so during the decision window's 0.16× dilation five or six
//! frames share one tick. A decision made on any of them must survive to that
//! tick — the latch is what makes the slow-motion window feel responsive
//! instead of eating inputs.

use axiom::prelude::Vec2;
use axiom_input::{ActionId, DeviceFrame, InputState, KeyToken};
use axiom_kernel::Tick;

use crate::showcase::DiagnosticCommand;

/// Gameplay actions.
const ACTION_SCRAMBLE: ActionId = ActionId::new(1);
const ACTION_RESET: ActionId = ActionId::new(2);
const ACTION_PRIMARY: ActionId = ActionId::new(9);
const ACTION_UP: ActionId = ActionId::new(10);
const ACTION_DOWN: ActionId = ActionId::new(11);
const ACTION_LEFT: ActionId = ActionId::new(12);
const ACTION_RIGHT: ActionId = ActionId::new(13);
const ACTION_READ_ONE: ActionId = ActionId::new(14);
const ACTION_READ_TWO: ActionId = ActionId::new(15);
const ACTION_READ_THREE: ActionId = ActionId::new(16);
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
///
/// `stick_x` / `stick_y` are fed by a **gamepad** only — there is no on-screen
/// joystick, because the prototype does not ask the player to steer (see
/// `web/touch.rs`). The four answers arrive as `read` / `scramble`.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct TouchInput {
    pub stick_x: f32,
    pub stick_y: f32,
    pub primary: bool,
    pub reset: bool,
    /// A tapped decision read, `0..3`.
    pub read: Option<usize>,
    /// The scramble control was tapped.
    pub scramble: bool,
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
        state.bind_action(ACTION_SCRAMBLE, &[KeyToken::new("Space")]);
        state.bind_action(ACTION_RESET, &[KeyToken::new("KeyR")]);
        state.bind_action(ACTION_READ_ONE, &[KeyToken::new("Digit1")]);
        state.bind_action(ACTION_READ_TWO, &[KeyToken::new("Digit2")]);
        state.bind_action(ACTION_READ_THREE, &[KeyToken::new("Digit3")]);
        state.bind_action(ACTION_CAM_FORMATION, &[KeyToken::new("F2")]);
        state.bind_action(ACTION_CAM_QB, &[KeyToken::new("F3")]);
        state.bind_action(ACTION_CAM_FLIGHT, &[KeyToken::new("F4")]);
        state.bind_action(ACTION_CAM_CARRIER, &[KeyToken::new("F5")]);
        state.bind_action(ACTION_CAM_AUTO, &[KeyToken::new("F6")]);
        state.bind_action(ACTION_DEBUG, &[KeyToken::new("F1")]);
        state.bind_action(ACTION_PRIMARY, &[KeyToken::new("Enter")]);
        state.bind_action(
            ACTION_UP,
            &[KeyToken::new("KeyW"), KeyToken::new("ArrowUp")],
        );
        state.bind_action(
            ACTION_DOWN,
            &[KeyToken::new("KeyS"), KeyToken::new("ArrowDown")],
        );
        state.bind_action(
            ACTION_LEFT,
            &[KeyToken::new("KeyA"), KeyToken::new("ArrowLeft")],
        );
        state.bind_action(
            ACTION_RIGHT,
            &[KeyToken::new("KeyD"), KeyToken::new("ArrowRight")],
        );
        GameInput {
            state,
            sample_n: 0,
            pending: Vec::new(),
        }
    }

    /// Sample one rendered frame: latch its press edges and return the movement
    /// stick, offense-relative and clamped.
    pub fn sample(&mut self, size: Vec2, keys_down: &[KeyToken], touch: TouchInput) -> Vec2 {
        let frame = DeviceFrame::new(size, keys_down, &[]);
        self.state.sample(Tick::new(self.sample_n), &frame);
        self.sample_n += 1;

        let pressed: [(ActionId, DiagnosticCommand); 12] = [
            (ACTION_SCRAMBLE, DiagnosticCommand::Scramble),
            (ACTION_RESET, DiagnosticCommand::ResetAll),
            (ACTION_READ_ONE, DiagnosticCommand::ThrowRead(0)),
            (ACTION_READ_TWO, DiagnosticCommand::ThrowRead(1)),
            (ACTION_READ_THREE, DiagnosticCommand::ThrowRead(2)),
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
        // A tapped read is the exact same command the number keys emit, so the
        // decision path has one implementation regardless of device.
        if let Some(read) = touch.read {
            self.latch(DiagnosticCommand::ThrowRead(read.min(2)));
        }
        if touch.scramble {
            self.latch(DiagnosticCommand::Scramble);
        }

        let axis = |negative: ActionId, positive: ActionId| -> f32 {
            f32::from(self.state.is_down(positive)) - f32::from(self.state.is_down(negative))
        };
        Vec2::new(
            (touch.stick_x + axis(ACTION_LEFT, ACTION_RIGHT)).clamp(-1.0, 1.0),
            (touch.stick_y + axis(ACTION_DOWN, ACTION_UP)).clamp(-1.0, 1.0),
        )
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
