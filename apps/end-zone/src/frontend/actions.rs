//! The device-independent frontend action model. Every input device —
//! keyboard, gamepad, pointer, touch — translates into these actions at the
//! input layer; screens never inspect raw key codes. The frontend answers
//! with typed commands, audio intents, and haptic intents.

use crate::launch::MatchLaunchConfig;

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
/// frontend never touches the simulation itself.
#[derive(Debug, Clone, PartialEq)]
pub enum FrontendCommand {
    /// Freeze selections and boot the real showcase with this configuration.
    LaunchMatch(MatchLaunchConfig),
    /// Rebuild the current match from its original launch config + seed.
    RestartMatch,
    /// Dispose of the active match and return to the menu background sim.
    ReturnToMenu,
    /// Suspend / resume authoritative simulation advancement.
    SetPaused(bool),
}

/// Typed audio intents (mapped to procedural tones at the platform edge).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AudioIntent {
    Navigate,
    Confirm,
    Cancel,
    /// A rejected input (disabled item, duplicate team).
    Denied,
    TeamLock,
    VsImpact,
    Transition,
    PauseHit,
    ResumeRise,
}

/// Typed haptic intents. No Axiom host abstraction for vibration exists yet,
/// so these are recorded at the boundary and documented as unsupported — the
/// app never calls browser vibration APIs directly.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HapticIntent {
    Tick,
    Confirm,
    Impact,
}
