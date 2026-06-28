//! [`InputState`] — the one facade: a tick-indexed intent snapshot plus the
//! guard-free action-binding table that builds it.
//!
//! [`InputState::sample`] is the determinism boundary (SPEC-05 §6): it folds one
//! [`DeviceFrame`] of neutral device activity into the snapshot for a [`Tick`],
//! resolving bindings and computing **edges as pure tick arithmetic over the
//! down-sets** — `pressed`/`released` are the set-difference of this tick's
//! down-set against the previous tick's. Auto-repeat is suppressed structurally:
//! a held key is a `pressed` only on the single transition tick, because an edge
//! is a transition, not a level. The only time anywhere is the `Tick`; nothing
//! reads a clock, so the same `DeviceFrame` sequence reproduces byte-identical
//! snapshots and reads.

use axiom_kernel::Tick;
use axiom_math::Vec2;

use crate::action_id::ActionId;
use crate::device_frame::{DeviceFrame, Pointer};
use crate::key_token::KeyToken;
use crate::swipe_dir::SwipeDir;
use crate::swipe_synth::SwipeSynth;

/// One row of the binding table: the keys/buttons/gestures that fire an action.
#[derive(Debug, PartialEq)]
struct ActionBinding {
    action: ActionId,
    tokens: Vec<KeyToken>,
}

impl ActionBinding {
    /// Whether any token bound to this action is down in `frame`.
    fn is_down(&self, frame: &DeviceFrame) -> bool {
        self.tokens.iter().any(|token| frame.is_token_down(token))
    }
}

/// The resolved per-tick intent: the held/edge bitsets (indexed by binding row),
/// the most-recent down-edge tick per action, the pointer, the press position,
/// and any completed swipe. This — not the raw [`DeviceFrame`] — is the
/// tick-indexed stream that records, replays, and net-serializes.
#[derive(Debug, PartialEq)]
struct IntentSnapshot {
    down: Vec<bool>,
    pressed: Vec<bool>,
    released: Vec<bool>,
    last_press: Vec<Option<Tick>>,
    pointer: Option<Pointer>,
    pointer_pressed: Option<Vec2>,
    swipe: Option<SwipeDir>,
}

impl IntentSnapshot {
    const fn empty() -> Self {
        IntentSnapshot {
            down: Vec::new(),
            pressed: Vec::new(),
            released: Vec::new(),
            last_press: Vec::new(),
            pointer: None,
            pointer_pressed: None,
            swipe: None,
        }
    }
}

/// The per-tick intent snapshot the simulation reads, and the action-binding
/// table that produced it. The single public facade of `axiom-input`.
#[derive(Debug, PartialEq)]
pub struct InputState {
    bindings: Vec<ActionBinding>,
    snapshot: IntentSnapshot,
    prev_down: Vec<bool>,
    prev_pointer_down: bool,
    swipe_synth: SwipeSynth,
}

impl InputState {
    /// A fresh input state: no bindings, an empty snapshot, no gesture.
    pub const fn new() -> Self {
        InputState {
            bindings: Vec::new(),
            snapshot: IntentSnapshot::empty(),
            prev_down: Vec::new(),
            prev_pointer_down: false,
            swipe_synth: SwipeSynth::new(),
        }
    }

    /// Configure — or remap — which neutral `keys` fire `action`. The action's
    /// previous binding is replaced, so a remap leaves the id gameplay reads
    /// unchanged. An action is down when **any** of its bound tokens is down.
    pub fn bind_action(&mut self, action: ActionId, keys: &[KeyToken]) {
        self.bindings.retain(|binding| binding.action != action);
        self.bindings.push(ActionBinding {
            action,
            tokens: keys.to_vec(),
        });
    }

    /// Fold one frame of neutral device activity into the snapshot for `tick`:
    /// resolve every binding's down state, compute the press/release edges
    /// against the previous tick's down-set, stamp new down-edges with `tick`,
    /// and synthesize the pointer and swipe arms.
    pub fn sample(&mut self, tick: Tick, frame: &DeviceFrame) {
        let down: Vec<bool> = self
            .bindings
            .iter()
            .map(|binding| binding.is_down(frame))
            .collect();
        let pressed: Vec<bool> = down
            .iter()
            .enumerate()
            .map(|(i, &now)| now & !self.prev_down.get(i).copied().unwrap_or(false))
            .collect();
        let released: Vec<bool> = down
            .iter()
            .enumerate()
            .map(|(i, &now)| (!now) & self.prev_down.get(i).copied().unwrap_or(false))
            .collect();
        let last_press: Vec<Option<Tick>> = pressed
            .iter()
            .enumerate()
            .map(|(i, &edge)| {
                let previous = self.snapshot.last_press.get(i).copied().flatten();
                [previous, Some(tick)][usize::from(edge)]
            })
            .collect();

        let pointer = frame.primary_pointer();
        let pointer_down = pointer.map(|contact| contact.down).unwrap_or(false);
        let pointer_pressed = (pointer_down & !self.prev_pointer_down)
            .then(|| pointer.map(|contact| contact.pos))
            .flatten();
        let swipe = self
            .swipe_synth
            .fold(frame.surface(), frame.pointers())
            .map(SwipeDir::from_unit);

        self.snapshot = IntentSnapshot {
            down: down.clone(),
            pressed,
            released,
            last_press,
            pointer,
            pointer_pressed,
            swipe,
        };
        self.prev_down = down;
        self.prev_pointer_down = pointer_down;
    }

    /// The binding row index for `action`, or `None` when it is unbound.
    fn index_of(&self, action: ActionId) -> Option<usize> {
        self.bindings
            .iter()
            .position(|binding| binding.action == action)
    }

    /// Whether `action` is held this tick.
    pub fn is_down(&self, action: ActionId) -> bool {
        self.index_of(action)
            .and_then(|i| self.snapshot.down.get(i).copied())
            .unwrap_or(false)
    }

    /// Whether `action` had a down-edge this tick (no auto-repeat).
    pub fn pressed(&self, action: ActionId) -> bool {
        self.index_of(action)
            .and_then(|i| self.snapshot.pressed.get(i).copied())
            .unwrap_or(false)
    }

    /// Whether `action` had an up-edge this tick.
    pub fn released(&self, action: ActionId) -> bool {
        self.index_of(action)
            .and_then(|i| self.snapshot.released.get(i).copied())
            .unwrap_or(false)
    }

    /// A `-1 | 0 | 1` axis from two opposing actions: `+1` when only `pos` is
    /// held, `-1` when only `neg`, `0` when both or neither.
    pub fn axis(&self, neg: ActionId, pos: ActionId) -> i8 {
        i8::from(self.is_down(pos)) - i8::from(self.is_down(neg))
    }

    /// The primary contact this tick, or `None` when none was sampled.
    pub fn pointer(&self) -> Option<Pointer> {
        self.snapshot.pointer
    }

    /// The position the primary contact was pressed at this tick (its down-edge),
    /// or `None` when there was no press this tick.
    pub fn pointer_pressed(&self) -> Option<Vec2> {
        self.snapshot.pointer_pressed
    }

    /// The direction of a swipe completed this tick, or `None`.
    pub fn swipe(&self) -> Option<SwipeDir> {
        self.snapshot.swipe
    }

    /// The tick of `action`'s most recent down-edge, or `None` before any press.
    /// A reaction/rhythm game judges `tick - pressed_at_tick(action)` against a
    /// fixed tick window, identically across replays.
    pub fn pressed_at_tick(&self, action: ActionId) -> Option<Tick> {
        self.index_of(action)
            .and_then(|i| self.snapshot.last_press.get(i).copied().flatten())
    }
}

impl Default for InputState {
    fn default() -> Self {
        InputState::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const MOVE_LEFT: ActionId = ActionId::new(0);
    const MOVE_RIGHT: ActionId = ActionId::new(1);
    const JUMP: ActionId = ActionId::new(2);

    fn key(name: &str) -> KeyToken {
        KeyToken::new(name)
    }

    /// A keyboard-only frame: the named tokens down, no pointer.
    fn keys(down: &[&str]) -> DeviceFrame {
        let tokens: Vec<KeyToken> = down.iter().map(|name| key(name)).collect();
        DeviceFrame::new(Vec2::new(1000.0, 600.0), &tokens, &[])
    }

    fn bound_state() -> InputState {
        let mut input = InputState::new();
        input.bind_action(MOVE_LEFT, &[key("KeyA"), key("ArrowLeft")]);
        input.bind_action(MOVE_RIGHT, &[key("KeyD")]);
        input.bind_action(JUMP, &[key("Space")]);
        input
    }

    #[test]
    fn unbound_action_reads_are_all_neutral() {
        let input = InputState::new();
        assert!(!input.is_down(JUMP));
        assert!(!input.pressed(JUMP));
        assert!(!input.released(JUMP));
        assert_eq!(input.pressed_at_tick(JUMP), None);
        assert_eq!(input.axis(MOVE_LEFT, MOVE_RIGHT), 0);
    }

    #[test]
    fn default_equals_new_and_reads_neutral() {
        let input = InputState::default();
        assert_eq!(input, InputState::new());
        assert!(!input.is_down(JUMP));
    }

    #[test]
    fn any_bound_token_holds_the_action() {
        let mut input = bound_state();
        // The alternate token for MOVE_LEFT also holds it.
        input.sample(Tick::new(1), &keys(&["ArrowLeft"]));
        assert!(input.is_down(MOVE_LEFT));
        assert!(!input.is_down(MOVE_RIGHT));
    }

    #[test]
    fn a_held_key_presses_once_then_only_holds() {
        let mut input = bound_state();
        // Tick 1: down-edge — pressed and held, stamped at tick 1.
        input.sample(Tick::new(1), &keys(&["Space"]));
        assert!(input.pressed(JUMP));
        assert!(input.is_down(JUMP));
        assert!(!input.released(JUMP));
        assert_eq!(input.pressed_at_tick(JUMP), Some(Tick::new(1)));
        // Tick 2: still held — held, but NOT pressed again (auto-repeat suppressed).
        input.sample(Tick::new(2), &keys(&["Space"]));
        assert!(!input.pressed(JUMP));
        assert!(input.is_down(JUMP));
        // Stamp stays at the original down-edge tick.
        assert_eq!(input.pressed_at_tick(JUMP), Some(Tick::new(1)));
    }

    #[test]
    fn release_is_a_single_up_edge() {
        let mut input = bound_state();
        input.sample(Tick::new(1), &keys(&["Space"]));
        // Tick 2: lifted — released exactly once, no longer down.
        input.sample(Tick::new(2), &keys(&[]));
        assert!(input.released(JUMP));
        assert!(!input.is_down(JUMP));
        // Tick 3: still up — no release edge.
        input.sample(Tick::new(3), &keys(&[]));
        assert!(!input.released(JUMP));
    }

    #[test]
    fn press_restamps_on_a_later_press() {
        let mut input = bound_state();
        input.sample(Tick::new(5), &keys(&["Space"]));
        assert_eq!(input.pressed_at_tick(JUMP), Some(Tick::new(5)));
        input.sample(Tick::new(6), &keys(&[])); // release
        // Stamp persists through the up window.
        assert_eq!(input.pressed_at_tick(JUMP), Some(Tick::new(5)));
        input.sample(Tick::new(9), &keys(&["Space"])); // press again
        assert_eq!(input.pressed_at_tick(JUMP), Some(Tick::new(9)));
    }

    #[test]
    fn axis_returns_all_four_held_combinations() {
        let mut input = bound_state();
        input.sample(Tick::new(1), &keys(&[]));
        assert_eq!(input.axis(MOVE_LEFT, MOVE_RIGHT), 0); // neither
        input.sample(Tick::new(2), &keys(&["KeyD"]));
        assert_eq!(input.axis(MOVE_LEFT, MOVE_RIGHT), 1); // only pos
        input.sample(Tick::new(3), &keys(&["KeyA"]));
        assert_eq!(input.axis(MOVE_LEFT, MOVE_RIGHT), -1); // only neg
        input.sample(Tick::new(4), &keys(&["KeyA", "KeyD"]));
        assert_eq!(input.axis(MOVE_LEFT, MOVE_RIGHT), 0); // both
    }

    #[test]
    fn rebinding_replaces_the_keys_for_an_action() {
        let mut input = bound_state();
        // Remap JUMP from Space to KeyJ.
        input.bind_action(JUMP, &[key("KeyJ")]);
        input.sample(Tick::new(1), &keys(&["Space"]));
        assert!(!input.is_down(JUMP)); // old key no longer fires it
        input.sample(Tick::new(2), &keys(&["KeyJ"]));
        assert!(input.is_down(JUMP)); // new key does
    }

    #[test]
    fn pointer_and_pointer_pressed_track_the_primary_contact() {
        let mut input = InputState::new();
        let surface = Vec2::new(1000.0, 600.0);
        // No samples: no pointer, no press.
        input.sample(Tick::new(1), &DeviceFrame::new(surface, &[], &[]));
        assert_eq!(input.pointer(), None);
        assert_eq!(input.pointer_pressed(), None);
        // Down-edge: pointer reported and press position is this tick's.
        let frame = DeviceFrame::new(surface, &[], &[(Vec2::new(120.0, 80.0), true)]);
        input.sample(Tick::new(2), &frame);
        assert_eq!(
            input.pointer(),
            Some(Pointer {
                pos: Vec2::new(120.0, 80.0),
                down: true,
            })
        );
        assert_eq!(input.pointer_pressed(), Some(Vec2::new(120.0, 80.0)));
        // Still down next tick: a hold is not a fresh press.
        let held = DeviceFrame::new(surface, &[], &[(Vec2::new(130.0, 90.0), true)]);
        input.sample(Tick::new(3), &held);
        assert_eq!(input.pointer_pressed(), None);
    }

    #[test]
    fn a_hovering_pointer_is_reported_but_not_pressed() {
        let mut input = InputState::new();
        let surface = Vec2::new(1000.0, 600.0);
        let hover = DeviceFrame::new(surface, &[], &[(Vec2::new(50.0, 50.0), false)]);
        input.sample(Tick::new(1), &hover);
        assert_eq!(
            input.pointer(),
            Some(Pointer {
                pos: Vec2::new(50.0, 50.0),
                down: false,
            })
        );
        assert_eq!(input.pointer_pressed(), None);
    }

    #[test]
    fn a_completed_drag_reads_as_a_swipe_direction() {
        let mut input = InputState::new();
        let surface = Vec2::new(1000.0, 600.0);
        input.sample(
            Tick::new(1),
            &DeviceFrame::new(surface, &[], &[(Vec2::new(200.0, 300.0), true)]),
        );
        assert_eq!(input.swipe(), None); // mid-gesture
        input.sample(
            Tick::new(2),
            &DeviceFrame::new(surface, &[], &[(Vec2::new(400.0, 300.0), true)]),
        );
        input.sample(Tick::new(3), &DeviceFrame::new(surface, &[], &[])); // lift
        assert_eq!(input.swipe(), Some(SwipeDir::Right));
    }

    #[test]
    fn the_same_frame_stream_reproduces_byte_identical_state() {
        let frames = [
            keys(&["Space"]),
            keys(&["Space", "KeyD"]),
            keys(&["KeyD"]),
            keys(&[]),
        ];
        let drive = || {
            let mut input = bound_state();
            frames
                .iter()
                .enumerate()
                .for_each(|(i, frame)| input.sample(Tick::new(i as u64 + 1), frame));
            input
        };
        // Two independent runs of the same stream are equal in their entirety.
        assert_eq!(drive(), drive());
    }

    #[test]
    fn a_different_stream_diverges() {
        let mut a = bound_state();
        let mut b = bound_state();
        a.sample(Tick::new(1), &keys(&["Space"]));
        b.sample(Tick::new(1), &keys(&["KeyD"]));
        assert_ne!(a, b);
    }
}
