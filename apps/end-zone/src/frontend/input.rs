//! The frontend input translator: one neutral per-frame device snapshot in,
//! device-independent [`DeviceAction`]s out — with edge detection, a
//! navigation repeat delay + cadence, and the stable last-active-device
//! policy that keeps navigation hints from flickering.

use super::actions::{DeviceAction, FrontendAction, InputDevice, NavDirection};
use super::bindings::{BindableAction, ControlBindings};

/// One frame of neutral device state gathered by the platform edge.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct FrontendInputFrame {
    /// Keyboard `KeyboardEvent.code` tokens currently held.
    pub keys_down: Vec<String>,
    /// Gamepad tokens currently held (`PadA`, `PadUp`, … — includes stick
    /// directions already thresholded by the edge).
    pub pad_down: Vec<String>,
    /// Pointer position in logical UI pixels.
    pub pointer: Option<(f32, f32)>,
    /// Pointer press edge this frame.
    pub pointer_pressed: bool,
    /// Whether the pointer is a touch contact (drives touch hints).
    pub pointer_is_touch: bool,
}

impl FrontendInputFrame {
    fn token_down(&self, token: &str) -> Option<InputDevice> {
        if self.keys_down.iter().any(|t| t == token) {
            return Some(InputDevice::Keyboard);
        }
        if self.pad_down.iter().any(|t| t == token) {
            return Some(InputDevice::Gamepad);
        }
        None
    }

    /// Every token newly present vs `previous` (for rebind capture).
    pub fn newly_pressed(&self, previous: &FrontendInputFrame) -> Vec<String> {
        let mut fresh = Vec::new();
        for token in self.keys_down.iter().chain(self.pad_down.iter()) {
            let was = previous.keys_down.iter().any(|t| t == token)
                || previous.pad_down.iter().any(|t| t == token);
            if !was && !fresh.contains(token) {
                fresh.push(token.clone());
            }
        }
        fresh
    }
}

/// Ticks a held direction waits before repeating, then between repeats.
pub const REPEAT_DELAY: u32 = 18;
pub const REPEAT_CADENCE: u32 = 7;

/// How many consecutive pointer-move frames flip the hint device to pointer.
const POINTER_HINT_THRESHOLD: u32 = 5;

const DIRECTIONS: [(BindableAction, NavDirection); 4] = [
    (BindableAction::NavUp, NavDirection::Up),
    (BindableAction::NavDown, NavDirection::Down),
    (BindableAction::NavLeft, NavDirection::Left),
    (BindableAction::NavRight, NavDirection::Right),
];

const EDGES: [BindableAction; 3] = [
    BindableAction::Confirm,
    BindableAction::Cancel,
    BindableAction::Pause,
];

/// The stateful translator.
#[derive(Debug, Clone)]
pub struct InputTranslator {
    previous: FrontendInputFrame,
    /// Held ticks per direction (0 = not held).
    held: [u32; 4],
    hint_device: InputDevice,
    pointer_streak: u32,
    last_pointer: Option<(f32, f32)>,
}

impl Default for InputTranslator {
    fn default() -> Self {
        InputTranslator {
            previous: FrontendInputFrame::default(),
            held: [0; 4],
            hint_device: InputDevice::Keyboard,
            pointer_streak: 0,
            last_pointer: None,
        }
    }
}

impl InputTranslator {
    pub fn new() -> Self {
        InputTranslator::default()
    }

    /// The stable device navigation hints should target.
    pub fn hint_device(&self) -> InputDevice {
        self.hint_device
    }

    /// Translate one frame into ordered actions.
    pub fn tick(
        &mut self,
        frame: &FrontendInputFrame,
        bindings: &ControlBindings,
    ) -> Vec<DeviceAction> {
        let mut actions = Vec::new();

        // Directional navigation with repeat.
        for (index, (action, direction)) in DIRECTIONS.into_iter().enumerate() {
            let device = self.action_device(frame, bindings, action);
            match device {
                Some(device) => {
                    let held = self.held[index];
                    let fire = held == 0
                        || (held >= REPEAT_DELAY && (held - REPEAT_DELAY) % REPEAT_CADENCE == 0);
                    if fire {
                        actions.push(DeviceAction::new(
                            FrontendAction::Navigate(direction),
                            device,
                        ));
                        self.note_device(device);
                    }
                    self.held[index] = held.saturating_add(1);
                }
                None => self.held[index] = 0,
            }
        }

        // Confirm / cancel / pause edges.
        for action in EDGES {
            if let Some(device) = self.edge_device(frame, bindings, action) {
                let mapped = match action {
                    BindableAction::Confirm => FrontendAction::Confirm,
                    BindableAction::Cancel => FrontendAction::Cancel,
                    _ => FrontendAction::Pause,
                };
                actions.push(DeviceAction::new(mapped, device));
                self.note_device(device);
            }
        }

        // Pointer motion + activation.
        let pointer_device = if frame.pointer_is_touch {
            InputDevice::Touch
        } else {
            InputDevice::Pointer
        };
        if let Some((x, y)) = frame.pointer {
            let moved = self
                .last_pointer
                .map(|(px, py)| (px - x).abs() + (py - y).abs() > 1.5)
                .unwrap_or(true);
            if moved {
                actions.push(DeviceAction::new(
                    FrontendAction::PointerMove { x, y },
                    pointer_device,
                ));
                self.pointer_streak = self.pointer_streak.saturating_add(1);
                if self.pointer_streak >= POINTER_HINT_THRESHOLD {
                    self.hint_device = pointer_device;
                }
            }
            if frame.pointer_pressed {
                actions.push(DeviceAction::new(
                    FrontendAction::PointerActivate { x, y },
                    pointer_device,
                ));
                self.hint_device = pointer_device;
            }
            self.last_pointer = Some((x, y));
        }

        self.previous = frame.clone();
        actions
    }

    /// Tokens newly pressed this frame (rebind capture channel).
    pub fn captured_tokens(&self, frame: &FrontendInputFrame) -> Vec<String> {
        frame.newly_pressed(&self.previous)
    }

    fn action_device(
        &self,
        frame: &FrontendInputFrame,
        bindings: &ControlBindings,
        action: BindableAction,
    ) -> Option<InputDevice> {
        bindings
            .tokens(action)
            .iter()
            .find_map(|token| frame.token_down(token))
            .or_else(|| emergency_tokens(action).find_map(|token| frame.token_down(token)))
    }

    fn edge_device(
        &self,
        frame: &FrontendInputFrame,
        bindings: &ControlBindings,
        action: BindableAction,
    ) -> Option<InputDevice> {
        let now = self.action_device(frame, bindings, action);
        let before = bindings
            .tokens(action)
            .iter()
            .any(|token| self.previous.token_down(token).is_some())
            || emergency_tokens(action).any(|token| self.previous.token_down(token).is_some());
        now.filter(|_| !before)
    }

    fn note_device(&mut self, device: InputDevice) {
        // Discrete (non-pointer) actions flip the hint immediately; pointer
        // motion needs a streak (handled above), so analog noise or a single
        // stray event never oscillates the hints.
        if matches!(device, InputDevice::Keyboard | InputDevice::Gamepad) {
            self.hint_device = device;
            self.pointer_streak = 0;
        }
    }
}

fn emergency_tokens(action: BindableAction) -> impl Iterator<Item = &'static str> {
    let list: &[&str] = match action {
        BindableAction::Confirm => &["Enter"],
        BindableAction::Cancel => &["Escape"],
        BindableAction::NavUp => &["ArrowUp"],
        BindableAction::NavDown => &["ArrowDown"],
        BindableAction::NavLeft => &["ArrowLeft"],
        BindableAction::NavRight => &["ArrowRight"],
        _ => &[],
    };
    list.iter().copied()
}
