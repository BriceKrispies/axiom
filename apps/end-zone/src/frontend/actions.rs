//! The device-independent frontend action model. Every input device —
//! keyboard, gamepad, pointer, touch — translates into these actions at the
//! input layer; screens never inspect raw key codes. The frontend answers with
//! typed lifecycle commands and audio intents.

/// A navigation direction.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NavDirection {
    Up,
    Down,
    Left,
    Right,
}

/// One device-independent frontend action.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum FrontendAction {
    Navigate(NavDirection),
    Confirm,
    Cancel,
    Pause,
    /// Pointer moved to logical UI coordinates.
    PointerMove {
        x: f32,
        y: f32,
    },
    /// Pointer / touch pressed at logical UI coordinates.
    PointerActivate {
        x: f32,
        y: f32,
    },
}

/// Which physical device produced an action (drives navigation hints via a
/// stable last-active-device policy).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InputDevice {
    Keyboard,
    Gamepad,
    Pointer,
    Touch,
}

/// An action stamped with its source device.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DeviceAction {
    pub action: FrontendAction,
    pub device: InputDevice,
}

impl DeviceAction {
    pub fn new(action: FrontendAction, device: InputDevice) -> Self {
        DeviceAction { action, device }
    }
}

/// Typed lifecycle commands the frontend hands the composition layer. The
/// frontend never touches the simulation itself; it only names what should
/// happen to the run.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FrontendCommand {
    /// Boot a fresh run with this explicit deterministic seed (title START and
    /// game-over PLAY AGAIN). The shell resolves the seed into a `RunConfig`.
    LaunchRun { seed: u64 },
    /// Rebuild the active run from its original `RunConfig` (pause RESTART RUN).
    RestartRun,
    /// Dispose of the active run and return to the ambient title showcase.
    ReturnToTitle,
    /// Suspend / resume authoritative simulation advancement.
    SetPaused(bool),
}

/// Typed audio intents (mapped to procedural tones at the platform edge).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AudioIntent {
    Navigate,
    Confirm,
    Cancel,
    /// A rejected input (disabled item).
    Denied,
    Transition,
    PauseHit,
    ResumeRise,
}
