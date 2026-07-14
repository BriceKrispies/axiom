//! Gamepad polling: standard-mapping pads become neutral `Pad*` tokens (the
//! frontend's binding vocabulary) plus a deadzoned analog stick vector for
//! in-match movement. Nothing below the edge knows a gamepad exists.

use wasm_bindgen::JsCast;
use web_sys::{Gamepad, GamepadButton};

/// Analog threshold that registers a stick direction as a d-pad token.
const NAV_THRESHOLD: f64 = 0.5;
/// Analog deadzone for the raw movement vector.
const DEADZONE: f64 = 0.2;

/// One frame of gamepad state.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct PadState {
    /// Held `Pad*` tokens (buttons + thresholded stick directions).
    pub tokens: Vec<String>,
    /// Deadzoned left stick, `x` right, `y` up, each `-1..=1`.
    pub stick: (f32, f32),
}

/// Standard-mapping button index → token.
const BUTTONS: [(u32, &str); 9] = [
    (0, "PadA"),
    (1, "PadB"),
    (2, "PadX"),
    (3, "PadY"),
    (9, "PadStart"),
    (12, "PadUp"),
    (13, "PadDown"),
    (14, "PadLeft"),
    (15, "PadRight"),
];

/// Poll every connected gamepad into one merged state.
pub fn poll() -> PadState {
    let mut state = PadState::default();
    let Some(window) = web_sys::window() else {
        return state;
    };
    let Ok(pads) = window.navigator().get_gamepads() else {
        return state;
    };
    for value in pads.iter() {
        let Ok(pad) = value.dyn_into::<Gamepad>() else {
            continue;
        };
        let buttons = pad.buttons();
        for (index, token) in BUTTONS {
            let pressed = buttons
                .get(index)
                .dyn_into::<GamepadButton>()
                .map(|b| b.pressed())
                .unwrap_or(false);
            if pressed && !state.tokens.iter().any(|t| t == token) {
                state.tokens.push(token.to_string());
            }
        }
        let axes = pad.axes();
        let x = axes.get(0).as_f64().unwrap_or(0.0);
        let y = axes.get(1).as_f64().unwrap_or(0.0);
        let mut push = |token: &str| {
            if !state.tokens.iter().any(|t| t == token) {
                state.tokens.push(token.to_string());
            }
        };
        if x < -NAV_THRESHOLD {
            push("PadLeft");
        }
        if x > NAV_THRESHOLD {
            push("PadRight");
        }
        if y < -NAV_THRESHOLD {
            push("PadUp");
        }
        if y > NAV_THRESHOLD {
            push("PadDown");
        }
        if x.abs() > DEADZONE || y.abs() > DEADZONE {
            // Browser stick `y` is down-positive; the game's stick is up.
            state.stick = (x as f32, -(y as f32));
        }
    }
    state
}
