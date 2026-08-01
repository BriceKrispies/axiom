//! Turning raw device state into a [`DriveCommand`], through the engine's
//! deterministic action table.
//!
//! Keys never reach the simulation. A frame's held [`KeyToken`]s (and, on the
//! browser arm, the gamepad's analogue axes) are folded by
//! [`axiom_input::InputState`] into named **actions** at a tick boundary, and
//! the actions are what the command is built from. That indirection buys two
//! things worth having: the simulation names `Throttle`, not `"KeyW"`, so
//! rebinding is a table edit; and the whole fold is a pure function of
//! `(tick, DeviceFrame)`, so a recorded device stream replays exactly.
//!
//! The analogue channels are the reason a gamepad's triggers work at all: the
//! action table answers "is throttle down", and the gamepad supplies "throttle
//! is 0.62". [`Controls::command`] takes both and prefers whichever is asking
//! for more, so holding W and half-pressing the trigger does the obvious thing.

use axiom_input::{DeviceFrame, InputState, KeyToken, Tick};
use axiom_math::Vec2;

use crate::command::DriveCommand;

/// The actions the game reads. The raw ids are stable and are what a rebinding
/// UI would key on.
pub mod action {
    use axiom_input::ActionId;

    pub const THROTTLE: ActionId = ActionId::new(1);
    pub const BRAKE: ActionId = ActionId::new(2);
    pub const STEER_LEFT: ActionId = ActionId::new(3);
    pub const STEER_RIGHT: ActionId = ActionId::new(4);
    pub const HANDBRAKE: ActionId = ActionId::new(5);
    pub const BOOST: ActionId = ActionId::new(6);
    pub const RESET: ActionId = ActionId::new(7);
    pub const PAUSE: ActionId = ActionId::new(8);
    pub const RESTART: ActionId = ActionId::new(9);
    pub const DEBUG: ActionId = ActionId::new(10);
}

/// Analogue axes a gamepad supplies alongside the digital actions. All default
/// to zero, which is exactly what a keyboard-only frame provides.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct AnalogueInput {
    /// Right trigger, `0..1`.
    pub throttle: f32,
    /// Left trigger, `0..1`.
    pub brake: f32,
    /// Left stick X, `-1..1`.
    pub steer: f32,
}

impl AnalogueInput {
    /// Clamp every channel into range and drop non-finite values.
    pub fn sanitised(self) -> AnalogueInput {
        let finite = |v: f32| if v.is_finite() { v } else { 0.0 };
        AnalogueInput {
            throttle: finite(self.throttle).clamp(0.0, 1.0),
            brake: finite(self.brake).clamp(0.0, 1.0),
            steer: finite(self.steer).clamp(-1.0, 1.0),
        }
    }
}

/// The set of keys physically held right now.
///
/// This exists as a tested model rather than as three lines in the browser edge
/// because getting it wrong is invisible and catastrophic: a key that is
/// recorded as pressed and never recorded as released is held **forever**, and
/// no amount of resetting the car will help, because the car is not what is
/// stuck.
///
/// ## Keys are identified by `code`, never by `key`
///
/// A browser keyboard event carries two names for the same physical key.
/// `code` ("KeyD") is the physical key and never changes. `key` ("d") is the
/// character it produced *this time*, and it changes with the modifiers — the
/// same key reports `"d"` on the way down and `"D"` on the way up if Shift went
/// down in between.
///
/// Recording both, as the browser edge originally did, therefore leaves a
/// permanent phantom: press D, press Shift, release D, and `"d"` is still held.
/// In a game where Shift is *boost* and D is *steer right*, that is a car that
/// steers right forever and cannot be reset out of it. So a key gets exactly one
/// identity, and it is the one that cannot change underneath us.
#[derive(Debug, Default, Clone, PartialEq)]
pub struct HeldKeys {
    keys: Vec<String>,
}

impl HeldKeys {
    /// An empty set.
    pub fn new() -> HeldKeys {
        HeldKeys::default()
    }

    /// The single identity a physical key is tracked by.
    ///
    /// `code` where the browser gives one; `key` only as a fallback for
    /// synthetic events (an on-screen keypad dispatching `key`-only events has
    /// no physical key to name).
    fn identity<'a>(code: &'a str, key: &'a str) -> &'a str {
        if code.is_empty() {
            key
        } else {
            code
        }
    }

    /// Record a key going down. Pressing an already-held key is a no-op, so
    /// auto-repeat cannot double-register.
    pub fn press(&mut self, code: &str, key: &str) {
        let id = Self::identity(code, key);
        if id.is_empty() {
            return;
        }
        if !self.keys.iter().any(|k| k == id) {
            self.keys.push(id.to_string());
        }
    }

    /// Record a key coming up.
    pub fn release(&mut self, code: &str, key: &str) {
        let id = Self::identity(code, key);
        self.keys.retain(|k| k != id);
    }

    /// Drop everything.
    ///
    /// Called when the window loses focus or is hidden. Without it, alt-tabbing
    /// mid-corner leaves the steering key held for the rest of the session: the
    /// browser delivers the `keydown` and then never delivers the matching
    /// `keyup`, because by then the page is not listening.
    pub fn clear(&mut self) {
        self.keys.clear();
    }

    /// Whether anything is held.
    pub fn is_empty(&self) -> bool {
        self.keys.is_empty()
    }

    /// How many keys are held.
    pub fn len(&self) -> usize {
        self.keys.len()
    }

    /// The held keys as engine key tokens.
    pub fn tokens(&self) -> Vec<KeyToken> {
        self.keys.iter().map(|k| KeyToken::new(k)).collect()
    }
}

/// The bound action table and the per-tick fold.
#[derive(Debug)]
pub struct Controls {
    state: InputState,
    tick: u64,
}

impl Controls {
    /// The default bindings.
    ///
    /// Both the letter keys and the arrow keys drive, because a racing game that
    /// only accepts one of them is a racing game half its players cannot steer.
    pub fn new() -> Controls {
        let mut state = InputState::new();
        state.bind_action(
            action::THROTTLE,
            &[key("KeyW"), key("ArrowUp"), key("w"), key("W")],
        );
        state.bind_action(
            action::BRAKE,
            &[key("KeyS"), key("ArrowDown"), key("s"), key("S")],
        );
        state.bind_action(
            action::STEER_LEFT,
            &[key("KeyA"), key("ArrowLeft"), key("a"), key("A")],
        );
        state.bind_action(
            action::STEER_RIGHT,
            &[key("KeyD"), key("ArrowRight"), key("d"), key("D")],
        );
        state.bind_action(action::HANDBRAKE, &[key("Space"), key(" ")]);
        state.bind_action(
            action::BOOST,
            &[key("ShiftLeft"), key("ShiftRight"), key("Shift")],
        );
        state.bind_action(action::RESET, &[key("KeyR"), key("r"), key("R")]);
        state.bind_action(action::PAUSE, &[key("Escape"), key("KeyP"), key("p")]);
        state.bind_action(action::RESTART, &[key("Enter"), key("KeyT"), key("t")]);
        state.bind_action(action::DEBUG, &[key("F1"), key("Backquote"), key("`")]);
        Controls { state, tick: 0 }
    }

    /// Fold one frame of held keys plus analogue axes into a command.
    ///
    /// The edge-triggered actions (reset, pause, restart, debug) use the action
    /// table's *press* edge rather than its held state, so holding the key does
    /// not fire them every frame.
    pub fn command(&mut self, keys: &[KeyToken], analogue: AnalogueInput) -> DriveCommand {
        self.tick += 1;
        let frame = DeviceFrame::new(SURFACE, keys, &[]);
        self.state.sample(Tick::new(self.tick), &frame);

        let analogue = analogue.sanitised();
        let digital_steer = self.state.axis(action::STEER_LEFT, action::STEER_RIGHT) as f32;
        // Whichever source is asking for more wins, so a keyboard and a pad can
        // be used together without either cancelling the other.
        let steer = if digital_steer.abs() >= analogue.steer.abs() {
            digital_steer
        } else {
            analogue.steer
        };
        DriveCommand {
            throttle: bool_or_analogue(self.state.is_down(action::THROTTLE), analogue.throttle),
            brake: bool_or_analogue(self.state.is_down(action::BRAKE), analogue.brake),
            steer,
            handbrake: self.state.is_down(action::HANDBRAKE),
            boost: self.state.is_down(action::BOOST),
            reset: self.state.pressed(action::RESET),
            pause: self.state.pressed(action::PAUSE),
            restart: self.state.pressed(action::RESTART),
        }
    }

    /// Whether the debug overlay was toggled this frame.
    pub fn debug_pressed(&self) -> bool {
        self.state.pressed(action::DEBUG)
    }

    /// How many frames have been folded.
    pub const fn tick(&self) -> u64 {
        self.tick
    }
}

impl Default for Controls {
    fn default() -> Self {
        Controls::new()
    }
}

/// The larger of a digital press and an analogue axis.
fn bool_or_analogue(down: bool, analogue: f32) -> f32 {
    if down {
        1.0f32.max(analogue)
    } else {
        analogue
    }
}

/// A key token.
fn key(name: &str) -> KeyToken {
    KeyToken::new(name)
}

/// The nominal surface the device frame is measured against. The game reads no
/// pointer input, so this is only there to satisfy the frame's shape.
const SURFACE: Vec2 = Vec2 {
    x: crate::WIDTH as f32,
    y: crate::HEIGHT as f32,
};

#[cfg(test)]
mod tests {
    use super::*;

    fn held(names: &[&str]) -> Vec<KeyToken> {
        names.iter().map(|n| key(n)).collect()
    }

    /// The bug this model exists to prevent, reproduced exactly: hold D, press
    /// Shift, release D. The browser reports the release with an uppercase
    /// `key`, so anything tracking by `key` never sees the lowercase one go up
    /// and the car steers right for the rest of the session.
    #[test]
    fn releasing_a_key_while_shift_is_held_does_not_leave_it_stuck() {
        let mut held = HeldKeys::new();

        // D goes down unshifted: the browser reports key "d".
        held.press("KeyD", "d");
        assert_eq!(held.len(), 1);

        // Shift goes down.
        held.press("ShiftLeft", "Shift");
        assert_eq!(held.len(), 2);

        // D comes up WITH shift held: the browser now reports key "D".
        held.release("KeyD", "D");
        assert!(
            !held.tokens().iter().any(|t| *t == KeyToken::new("d")),
            "the lowercase name must not survive the release"
        );
        assert!(
            !held.tokens().iter().any(|t| *t == KeyToken::new("KeyD")),
            "and neither must the physical one"
        );
        assert_eq!(held.len(), 1, "only shift is still down");

        held.release("ShiftLeft", "Shift");
        assert!(held.is_empty());
    }

    /// And the same thing through the whole command path: after that sequence
    /// the car must not still be being told to steer.
    #[test]
    fn a_shift_release_sequence_leaves_no_residual_steering() {
        let mut held = HeldKeys::new();
        let mut controls = Controls::new();

        held.press("KeyW", "w");
        held.press("KeyD", "d");
        assert!(controls.command(&held.tokens(), AnalogueInput::default()).steer > 0.0);

        held.press("ShiftLeft", "Shift");
        held.release("KeyD", "D");
        let after = controls.command(&held.tokens(), AnalogueInput::default());
        assert_eq!(after.steer, 0.0, "the wheel is straight again");
        assert_eq!(after.throttle, 1.0, "and the throttle is still held");
        assert!(after.boost, "and boost is on");
    }

    #[test]
    fn a_key_is_tracked_by_its_physical_code_not_its_character() {
        let mut held = HeldKeys::new();
        held.press("KeyA", "a");
        assert_eq!(held.len(), 1, "one physical key is one entry, not two");
        assert_eq!(held.tokens(), vec![KeyToken::new("KeyA")]);
        // A different character from the same physical key still releases it.
        held.release("KeyA", "A");
        assert!(held.is_empty());
    }

    /// A synthetic event (an on-screen keypad) has no physical key to name, so
    /// the character is all there is — and it must still work.
    #[test]
    fn a_synthetic_key_only_event_still_presses_and_releases() {
        let mut held = HeldKeys::new();
        held.press("", "w");
        assert_eq!(held.tokens(), vec![KeyToken::new("w")]);
        held.release("", "w");
        assert!(held.is_empty());
        // A wholly empty event is ignored rather than held forever.
        held.press("", "");
        assert!(held.is_empty());
    }

    #[test]
    fn auto_repeat_cannot_double_register_a_key() {
        let mut held = HeldKeys::new();
        for _ in 0..10 {
            held.press("KeyW", "w");
        }
        assert_eq!(held.len(), 1);
        held.release("KeyW", "w");
        assert!(held.is_empty(), "one release clears one press");
    }

    /// Losing focus mid-corner must not leave the wheel turned.
    #[test]
    fn clearing_releases_everything() {
        let mut held = HeldKeys::new();
        held.press("KeyD", "d");
        held.press("KeyW", "w");
        held.press("ShiftLeft", "Shift");
        assert_eq!(held.len(), 3);

        held.clear();
        assert!(held.is_empty());

        let mut controls = Controls::new();
        assert_eq!(
            controls.command(&held.tokens(), AnalogueInput::default()),
            DriveCommand::IDLE,
            "and the car is asked for nothing at all"
        );
    }

    #[test]
    fn releasing_a_key_that_was_never_held_is_harmless() {
        let mut held = HeldKeys::new();
        held.release("KeyQ", "q");
        assert!(held.is_empty());
        held.press("KeyW", "w");
        held.release("KeyQ", "q");
        assert_eq!(held.len(), 1);
    }

    #[test]
    fn nothing_held_asks_for_nothing() {
        let mut controls = Controls::new();
        assert_eq!(
            controls.command(&[], AnalogueInput::default()),
            DriveCommand::IDLE
        );
        assert!(!controls.debug_pressed());
    }

    #[test]
    fn the_letter_keys_and_the_arrow_keys_both_drive() {
        for keys in [held(&["KeyW"]), held(&["ArrowUp"]), held(&["w"])] {
            let mut controls = Controls::new();
            let command = controls.command(&keys, AnalogueInput::default());
            assert_eq!(command.throttle, 1.0, "{keys:?} did not accelerate");
        }
        for keys in [held(&["KeyA"]), held(&["ArrowLeft"])] {
            let mut controls = Controls::new();
            assert!(controls.command(&keys, AnalogueInput::default()).steer < 0.0);
        }
        for keys in [held(&["KeyD"]), held(&["ArrowRight"])] {
            let mut controls = Controls::new();
            assert!(controls.command(&keys, AnalogueInput::default()).steer > 0.0);
        }
    }

    #[test]
    fn every_control_the_manual_lists_is_bound() {
        let mut controls = Controls::new();
        let all = controls.command(
            &held(&["KeyW", "KeyS", "KeyD", "Space", "ShiftLeft", "KeyR", "Escape", "Enter"]),
            AnalogueInput::default(),
        );
        assert_eq!(all.throttle, 1.0);
        assert_eq!(all.brake, 1.0);
        assert!(all.steer > 0.0);
        assert!(all.handbrake, "space is the handbrake");
        assert!(all.boost, "shift is boost");
        assert!(all.reset, "R resets");
        assert!(all.pause, "escape pauses");
        assert!(all.restart, "enter restarts");
    }

    #[test]
    fn opposite_steering_keys_cancel() {
        let mut controls = Controls::new();
        let command = controls.command(&held(&["KeyA", "KeyD"]), AnalogueInput::default());
        assert_eq!(command.steer, 0.0);
    }

    /// Edge-triggered actions must fire once per press, not once per frame, or
    /// holding R would reset the car sixty times a second.
    #[test]
    fn edge_actions_fire_once_per_press() {
        let mut controls = Controls::new();
        let keys = held(&["KeyR", "Escape", "Enter"]);
        let first = controls.command(&keys, AnalogueInput::default());
        assert!(first.reset && first.pause && first.restart);

        let second = controls.command(&keys, AnalogueInput::default());
        assert!(!second.reset, "holding R does not re-reset");
        assert!(!second.pause);
        assert!(!second.restart);

        // Release and press again.
        controls.command(&[], AnalogueInput::default());
        let again = controls.command(&keys, AnalogueInput::default());
        assert!(again.reset, "a fresh press fires again");
    }

    #[test]
    fn held_actions_stay_held() {
        let mut controls = Controls::new();
        let keys = held(&["KeyW", "Space", "ShiftLeft"]);
        for _ in 0..10 {
            let command = controls.command(&keys, AnalogueInput::default());
            assert_eq!(command.throttle, 1.0);
            assert!(command.handbrake);
            assert!(command.boost);
        }
    }

    #[test]
    fn the_gamepad_triggers_drive_without_any_keys() {
        let mut controls = Controls::new();
        let command = controls.command(
            &[],
            AnalogueInput {
                throttle: 0.62,
                brake: 0.2,
                steer: -0.8,
            },
        );
        assert!((command.throttle - 0.62).abs() < 1.0e-6);
        assert!((command.brake - 0.2).abs() < 1.0e-6);
        assert!((command.steer + 0.8).abs() < 1.0e-6);
    }

    #[test]
    fn a_key_and_a_trigger_together_take_whichever_asks_for_more() {
        let mut controls = Controls::new();
        // Key fully down beats a half-pressed trigger.
        let command = controls.command(
            &held(&["KeyW", "KeyD"]),
            AnalogueInput {
                throttle: 0.4,
                brake: 0.0,
                steer: -0.3,
            },
        );
        assert_eq!(command.throttle, 1.0);
        assert_eq!(command.steer, 1.0, "the full digital steer wins");

        // A stick pushed further than the (absent) key wins instead.
        let mut controls = Controls::new();
        let command = controls.command(
            &[],
            AnalogueInput {
                throttle: 0.0,
                brake: 0.0,
                steer: -0.9,
            },
        );
        assert!((command.steer + 0.9).abs() < 1.0e-6);
    }

    #[test]
    fn analogue_input_is_clamped_and_never_non_finite() {
        let wild = AnalogueInput {
            throttle: 4.0,
            brake: -3.0,
            steer: f32::NAN,
        }
        .sanitised();
        assert_eq!(wild.throttle, 1.0);
        assert_eq!(wild.brake, 0.0);
        assert_eq!(wild.steer, 0.0);

        let mut controls = Controls::new();
        let command = controls.command(
            &[],
            AnalogueInput {
                throttle: f32::INFINITY,
                brake: f32::NAN,
                steer: 99.0,
            },
        );
        assert_eq!(command, command.sanitised(), "the command is already legal");
        assert_eq!(command.steer, 1.0);
    }

    #[test]
    fn the_debug_toggle_is_edge_triggered_and_separate_from_driving() {
        let mut controls = Controls::new();
        let command = controls.command(&held(&["F1"]), AnalogueInput::default());
        assert!(controls.debug_pressed());
        assert_eq!(command.throttle, 0.0, "F1 does not drive the car");
        controls.command(&held(&["F1"]), AnalogueInput::default());
        assert!(!controls.debug_pressed(), "holding it does not re-toggle");
    }

    #[test]
    fn the_fold_is_a_pure_function_of_the_key_sequence() {
        let sequence = [
            held(&["KeyW"]),
            held(&["KeyW", "KeyD"]),
            held(&["KeyW", "KeyD", "Space"]),
            held(&["KeyR"]),
            vec![],
        ];
        let run = || {
            let mut controls = Controls::new();
            sequence
                .iter()
                .map(|keys| controls.command(keys, AnalogueInput::default()))
                .collect::<Vec<_>>()
        };
        assert_eq!(run(), run());
    }

    #[test]
    fn the_tick_counter_advances_once_per_frame() {
        let mut controls = Controls::new();
        assert_eq!(controls.tick(), 0);
        controls.command(&[], AnalogueInput::default());
        controls.command(&[], AnalogueInput::default());
        assert_eq!(controls.tick(), 2);
        assert_eq!(Controls::default().tick(), 0);
    }
}
