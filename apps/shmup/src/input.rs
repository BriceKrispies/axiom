//! Ported from Claude-of-Duty `src/core/input.js:1-253` — the whole file.
//!
//! Input aggregation: keyboard, mouse (pointer-locked), and gamepad, exposed as
//! a stable per-frame snapshot so gameplay never touches raw DOM events.
//!
//! Edge queries ([`Input::pressed`], [`Input::released`]) are valid only during
//! the frame in which the transition happened — read them in the per-frame
//! update, never in a fixed substep. [`Input::begin_frame`] is what resolves the
//! queued DOM events into `down`/`pressed`/`released`, so "the frame" means
//! "since the last `begin_frame`".
//!
//! ## The DOM seam
//!
//! The source's constructor binds nine listeners in `attach()`. Every one of
//! them does exactly one thing: push a code onto a pending set, accumulate a
//! pointer delta, or flip `pointerLocked`. So the port splits the file at that
//! line: the whole snapshot/edge/curve model here is plain Rust with no browser
//! contact and is tested natively, and the listeners live in [`dom`], compiled
//! only for `wasm32`. That is the same split `crate::audio` (`web_audio`) and
//! `crate::ui` (each widget's `view`) already use.
//!
//! Two source calls reach for a browser global from inside otherwise-pure code,
//! and both are lifted to parameters rather than re-plumbed:
//!
//! * `beginFrame()` reads `this.config.sensitivity`/`invertY`. The port takes
//!   `&Config` as an argument instead of holding a copy, so the live config the
//!   settings menu edits is always the one applied — a stored copy would have to
//!   be invalidated by hand.
//! * `_pollGamepad()` calls `navigator.getGamepads()`. The port takes the four
//!   axes it actually reads (`axes[0..4]`) as an argument; [`dom`] is what reads
//!   `navigator`. Everything the source does with those axes — the dead zone and
//!   the cubic look curve — stays here, natively testable.

use std::collections::BTreeSet;

use crate::config::Config;
use crate::player::movement::{InputAction, PlayerInput};

/// One named action. `ACTIONS`, `input.js:9-27` — the seventeen keys of that
/// object literal, in source order.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Action {
    Forward,
    Back,
    Left,
    Right,
    Jump,
    Crouch,
    Prone,
    Sprint,
    Reload,
    Use,
    Melee,
    LeanLeft,
    LeanRight,
    SwapWeapon,
    Grenade,
    Flashlight,
    Pause,
}

/// `ACTIONS`. `input.js:9-27`. The codes are `KeyboardEvent.code` strings,
/// exactly as the source spells them.
pub const ACTIONS: [(Action, &[&str]); 17] = [
    (Action::Forward, &["KeyW", "ArrowUp"]),
    (Action::Back, &["KeyS", "ArrowDown"]),
    (Action::Left, &["KeyA", "ArrowLeft"]),
    (Action::Right, &["KeyD", "ArrowRight"]),
    (Action::Jump, &["Space"]),
    (Action::Crouch, &["ControlLeft", "KeyC"]),
    (Action::Prone, &["KeyZ"]),
    (Action::Sprint, &["ShiftLeft"]),
    (Action::Reload, &["KeyR"]),
    (Action::Use, &["KeyF"]),
    (Action::Melee, &["KeyV"]),
    (Action::LeanLeft, &["KeyQ"]),
    (Action::LeanRight, &["KeyE"]),
    (Action::SwapWeapon, &["Digit1", "Digit2", "Tab"]),
    (Action::Grenade, &["KeyG"]),
    (Action::Flashlight, &["KeyT"]),
    (Action::Pause, &["Escape"]),
];

impl Action {
    /// The codes bound to this action. `ACTIONS[name]`.
    pub fn codes(self) -> &'static [&'static str] {
        ACTIONS
            .iter()
            .find(|(a, _)| *a == self)
            .map(|(_, codes)| *codes)
            .unwrap_or(&[])
    }
}

/// `this.look` / `this._rawLook`. `input.js:41-42`.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Look {
    pub x: f64,
    pub y: f64,
}

/// `this.stick`. `input.js:53`.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Stick {
    pub move_x: f64,
    pub move_y: f64,
    pub look_x: f64,
    pub look_y: f64,
}

/// The gamepad dead zone. `input.js:181` — `Math.abs(v) < 0.16`.
pub const DEAD_ZONE: f64 = 0.16;

/// The look stick's response exponent. `input.js:185` — `Math.abs(v) ** 2.4`.
pub const LOOK_EXPONENT: f64 = 2.4;

/// `Math.sign`. Rust's `f64::signum` returns ±1 for ±0.0, where JS `Math.sign`
/// returns the zero itself — the divergence the port recipe names first. Every
/// `Math.sign` in this file goes through here.
fn js_sign(v: f64) -> f64 {
    f64::from(u8::from(v > 0.0)) - f64::from(u8::from(v < 0.0))
}

/// `class Input`. `input.js:29-253`.
///
/// The `canvas`/`config` constructor arguments do not survive: `canvas` is only
/// ever used by the two browser calls ([`dom`] owns them) and `config` is passed
/// per frame — see the module doc comment.
#[derive(Debug, Clone, Default)]
pub struct Input {
    /// `this.down` — codes currently held.
    down: BTreeSet<String>,
    /// `this._pressed` — went down this frame.
    pressed: BTreeSet<String>,
    /// `this._released` — went up this frame.
    released: BTreeSet<String>,
    pending_down: BTreeSet<String>,
    pending_up: BTreeSet<String>,

    /// `this.look` — the frame's pointer delta, in radians after sensitivity.
    pub look: Look,
    raw_look: Look,
    /// `this.wheel` — this frame's summed wheel sign.
    pub wheel: f64,
    pending_wheel: f64,

    pub pointer_locked: bool,
    pub enabled: bool,
    /// `this.frozen` — set by capture mode so scripted shots aren't fought by
    /// real input.
    pub frozen: bool,

    pub stick: Stick,
}

impl Input {
    /// `constructor(canvas, config)`. `input.js:30-65`.
    pub fn new() -> Self {
        Input {
            enabled: true,
            ..Input::default()
        }
    }

    /* ==================================================================== */
    /* the DOM events, as queued facts                                      */
    /* ==================================================================== */

    /// `_onKeyDown(e)`. `input.js:105-111`. `e.repeat` is filtered by the caller
    /// ([`dom`]) because it is a property of the event, not of this model.
    pub fn key_down(&mut self, code: &str) {
        if !self.enabled {
            return;
        }
        self.pending_down.insert(code.to_string());
    }

    /// `_onKeyUp(e)`. `input.js:113-116`.
    pub fn key_up(&mut self, code: &str) {
        if !self.enabled {
            return;
        }
        self.pending_up.insert(code.to_string());
    }

    /// `_onMouseDown(e)`. `input.js:118-122`. The source also requests pointer
    /// lock here; that is a browser call and lives in [`dom`], which asks
    /// [`Input::wants_pointer_lock`] whether to make it.
    pub fn mouse_down(&mut self, button: u16) {
        if !self.enabled {
            return;
        }
        self.pending_down.insert(format!("Mouse{button}"));
    }

    /// True when a left-button press should request pointer lock —
    /// `!this.pointerLocked && e.button === 0` (`input.js:120`).
    pub fn wants_pointer_lock(&self, button: u16) -> bool {
        self.enabled && !self.pointer_locked && button == 0
    }

    /// `_onMouseUp(e)`. `input.js:124-127`.
    pub fn mouse_up(&mut self, button: u16) {
        if !self.enabled {
            return;
        }
        self.pending_up.insert(format!("Mouse{button}"));
    }

    /// `_onMouseMove(e)`. `input.js:129-134`. `movement_x`/`movement_y` are
    /// already relative and unaffected by cursor clamping.
    pub fn mouse_move(&mut self, movement_x: f64, movement_y: f64) {
        if !self.enabled || !self.pointer_locked || self.frozen {
            return;
        }
        self.raw_look.x += movement_x;
        self.raw_look.y += movement_y;
    }

    /// `_onWheel(e)`. `input.js:136-139`.
    pub fn wheel_event(&mut self, delta_y: f64) {
        if !self.enabled {
            return;
        }
        self.pending_wheel += js_sign(delta_y);
    }

    /// `_onLockChange()`. `input.js:141-144`. Losing the lock blurs.
    pub fn set_pointer_locked(&mut self, locked: bool) {
        self.pointer_locked = locked;
        if !locked {
            self.blur();
        }
    }

    /// `_onBlur()`. `input.js:147-151`. Losing focus must release every held
    /// key, or the player runs forever.
    pub fn blur(&mut self) {
        let held: Vec<String> = self.down.iter().cloned().collect();
        self.pending_up.extend(held);
        self.raw_look.x = 0.0;
        self.raw_look.y = 0.0;
    }

    /* ==================================================================== */
    /* the per-frame snapshot                                               */
    /* ==================================================================== */

    /// `beginFrame()`. `input.js:153-175`. Resolves the queued events into the
    /// held/pressed/released sets, converts the accumulated raw pointer delta
    /// into this frame's `look`, latches the wheel, and polls the pad.
    ///
    /// `pad` is `navigator.getGamepads()`'s first pad's first four axes, or
    /// `None` when no pad is connected — see the module doc comment for why it
    /// is an argument.
    pub fn begin_frame(&mut self, config: &Config, pad: Option<[f64; 4]>) {
        self.pressed.clear();
        self.released.clear();

        for code in std::mem::take(&mut self.pending_down) {
            if self.down.insert(code.clone()) {
                self.pressed.insert(code);
            }
        }
        for code in std::mem::take(&mut self.pending_up) {
            if self.down.remove(&code) {
                self.released.insert(code);
            }
        }

        let s = f64::from(config.sensitivity);
        let invert = if config.invert_y { -1.0 } else { 1.0 };
        self.look.x = if self.frozen { 0.0 } else { self.raw_look.x * s };
        self.look.y = if self.frozen {
            0.0
        } else {
            self.raw_look.y * s * invert
        };
        self.raw_look.x = 0.0;
        self.raw_look.y = 0.0;

        self.wheel = self.pending_wheel;
        self.pending_wheel = 0.0;

        self.poll_gamepad(pad);
    }

    /// `endFrame()`. `input.js:177` — empty in the source, and empty here; it
    /// exists so the call site reads the same.
    pub fn end_frame(&self) {}

    /// `_pollGamepad()`. `input.js:179-190`, minus the `navigator` read.
    pub fn poll_gamepad(&mut self, pad: Option<[f64; 4]>) {
        let Some(axes) = pad else {
            self.stick = Stick::default();
            return;
        };
        let dz = |v: f64| {
            if v.abs() < DEAD_ZONE {
                0.0
            } else {
                (v - js_sign(v) * DEAD_ZONE) / (1.0 - DEAD_ZONE)
            }
        };
        self.stick.move_x = dz(axes[0]);
        self.stick.move_y = dz(axes[1]);
        // Cubic response curve on the look stick — fine aim near centre, fast
        // flicks at the edge.
        let curve = |v: f64| js_sign(v) * v.abs().powf(LOOK_EXPONENT);
        self.stick.look_x = curve(dz(axes[2]));
        self.stick.look_y = curve(dz(axes[3]));
    }

    /* ==================================================================== */
    /* queries                                                              */
    /* ==================================================================== */

    /// `action(name)`. `input.js:193-198`. True while any key bound to `action`
    /// is held.
    pub fn action(&self, action: Action) -> bool {
        action.codes().iter().any(|c| self.down.contains(*c))
    }

    /// `actionPressed(name)`. `input.js:200-205`.
    pub fn action_pressed(&self, action: Action) -> bool {
        action.codes().iter().any(|c| self.pressed.contains(*c))
    }

    /// `held(code)`. `input.js:207-209`.
    pub fn held(&self, code: &str) -> bool {
        self.down.contains(code)
    }

    /// `pressed(code)`. `input.js:211-213`.
    pub fn pressed(&self, code: &str) -> bool {
        self.pressed.contains(code)
    }

    /// `released(code)`. `input.js:215-217`.
    pub fn released(&self, code: &str) -> bool {
        self.released.contains(code)
    }

    /// `get fire()`. `input.js:219-221`.
    pub fn fire(&self) -> bool {
        self.down.contains("Mouse0")
    }

    /// `get firePressed()`. `input.js:223-225`.
    pub fn fire_pressed(&self) -> bool {
        self.pressed.contains("Mouse0")
    }

    /// `get ads()`. `input.js:227-229`.
    pub fn ads(&self) -> bool {
        self.down.contains("Mouse2")
    }

    /// `moveVector(out)`. `input.js:231-245`. Normalised WASD + left-stick
    /// movement, clamped to the unit disc so diagonals aren't faster than
    /// cardinals.
    pub fn move_vector(&self) -> (f64, f64) {
        let mut x = f64::from(u8::from(self.action(Action::Right)))
            - f64::from(u8::from(self.action(Action::Left)));
        let mut y = f64::from(u8::from(self.action(Action::Forward)))
            - f64::from(u8::from(self.action(Action::Back)));
        x += self.stick.move_x;
        y -= self.stick.move_y;
        let len = x.hypot(y);
        if len > 1.0 {
            x /= len;
            y /= len;
        }
        (x, y)
    }
}

/// The movement state machine's `ctx.input` seam
/// ([`crate::player::movement::PlayerInput`]), bound to the real input
/// snapshot. These four methods are exactly the four calls `movement.js`'s
/// `latchInput` makes.
impl PlayerInput for Input {
    fn move_vector(&self) -> (f64, f64) {
        Input::move_vector(self)
    }

    fn action(&self, action: InputAction) -> bool {
        Input::action(
            self,
            match action {
                InputAction::Jump => Action::Jump,
                InputAction::Crouch => Action::Crouch,
                InputAction::Prone => Action::Prone,
                InputAction::Sprint => Action::Sprint,
                InputAction::LeanLeft => Action::LeanLeft,
                InputAction::LeanRight => Action::LeanRight,
            },
        )
    }

    fn stick_move_y(&self) -> f64 {
        self.stick.move_y
    }

    fn ads(&self) -> bool {
        Input::ads(self)
    }
}

/// The browser listeners — `attach()`, `input.js:67-78`, plus the
/// `requestPointerLock` call `_onMouseDown` makes. Everything here pushes a
/// queued fact into a shared [`Input`] and decides nothing.
#[cfg(target_arch = "wasm32")]
pub mod dom {
    use std::cell::RefCell;
    use std::rc::Rc;

    use wasm_bindgen::prelude::*;
    use wasm_bindgen::JsCast;

    use super::Input;

    /// The shared snapshot the listeners write and the frame loop reads.
    pub type SharedInput = Rc<RefCell<Input>>;

    /// Read `navigator.getGamepads()`'s first pad's first four axes —
    /// `_pollGamepad`'s `navigator` half (`input.js:180-181`).
    pub fn poll_pad() -> Option<[f64; 4]> {
        let pads = web_sys::window()?.navigator().get_gamepads().ok()?;
        let pad = pads
            .iter()
            .filter_map(|p| p.dyn_into::<web_sys::Gamepad>().ok())
            .next()?;
        let axes = pad.axes();
        let at = |i: u32| axes.get(i).as_f64().unwrap_or(0.0);
        Some([at(0), at(1), at(2), at(3)])
    }

    /// `attach()`. `input.js:67-78`. The closures are `forget`ten deliberately:
    /// they live exactly as long as the page, which is what `detach()` would
    /// otherwise have to end, and this app never tears its input down.
    pub fn attach(input: &SharedInput, canvas: &web_sys::HtmlElement) {
        let window = web_sys::window().expect("a browser window");
        let document = window.document().expect("a document");

        fn on(target: &web_sys::EventTarget, name: &str, cb: Closure<dyn FnMut(web_sys::Event)>) {
            target
                .add_event_listener_with_callback(name, cb.as_ref().unchecked_ref())
                .expect("adding a listener to a live target");
            cb.forget();
        }

        let i = Rc::clone(input);
        on(
            window.as_ref(),
            "keydown",
            Closure::wrap(Box::new(move |e: web_sys::Event| {
                let e: web_sys::KeyboardEvent = e.unchecked_into();
                // Auto-repeat is not a new press (`input.js:107`).
                if e.repeat() {
                    return;
                }
                // Let devtools/refresh through; swallow everything else.
                if !e.meta_key() && !e.ctrl_key() {
                    e.prevent_default();
                }
                i.borrow_mut().key_down(&e.code());
            }) as Box<dyn FnMut(_)>),
        );

        let i = Rc::clone(input);
        on(
            window.as_ref(),
            "keyup",
            Closure::wrap(Box::new(move |e: web_sys::Event| {
                let e: web_sys::KeyboardEvent = e.unchecked_into();
                i.borrow_mut().key_up(&e.code());
            }) as Box<dyn FnMut(_)>),
        );

        let i = Rc::clone(input);
        let lock_target = canvas.clone();
        on(
            window.as_ref(),
            "mousedown",
            Closure::wrap(Box::new(move |e: web_sys::Event| {
                let e: web_sys::MouseEvent = e.unchecked_into();
                let button = e.button().max(0) as u16;
                // Chrome rejects the lock when the document is not eligible;
                // failing to lock is not a game error (`input.js:92-103`).
                if i.borrow().wants_pointer_lock(button) {
                    lock_target.request_pointer_lock();
                }
                i.borrow_mut().mouse_down(button);
            }) as Box<dyn FnMut(_)>),
        );

        let i = Rc::clone(input);
        on(
            window.as_ref(),
            "mouseup",
            Closure::wrap(Box::new(move |e: web_sys::Event| {
                let e: web_sys::MouseEvent = e.unchecked_into();
                i.borrow_mut().mouse_up(e.button().max(0) as u16);
            }) as Box<dyn FnMut(_)>),
        );

        let i = Rc::clone(input);
        on(
            window.as_ref(),
            "mousemove",
            Closure::wrap(Box::new(move |e: web_sys::Event| {
                let e: web_sys::MouseEvent = e.unchecked_into();
                i.borrow_mut()
                    .mouse_move(f64::from(e.movement_x()), f64::from(e.movement_y()));
            }) as Box<dyn FnMut(_)>),
        );

        let i = Rc::clone(input);
        on(
            window.as_ref(),
            "wheel",
            Closure::wrap(Box::new(move |e: web_sys::Event| {
                let e: web_sys::WheelEvent = e.unchecked_into();
                i.borrow_mut().wheel_event(e.delta_y());
            }) as Box<dyn FnMut(_)>),
        );

        let i = Rc::clone(input);
        on(
            window.as_ref(),
            "blur",
            Closure::wrap(Box::new(move |_e: web_sys::Event| {
                i.borrow_mut().blur();
            }) as Box<dyn FnMut(_)>),
        );

        let i = Rc::clone(input);
        let locked_to = canvas.clone();
        let doc = document.clone();
        on(
            document.as_ref(),
            "pointerlockchange",
            Closure::wrap(Box::new(move |_e: web_sys::Event| {
                let locked = doc
                    .pointer_lock_element()
                    .map(|el| el.is_same_node(Some(locked_to.as_ref())))
                    .unwrap_or(false);
                i.borrow_mut().set_pointer_locked(locked);
            }) as Box<dyn FnMut(_)>),
        );

        on(
            canvas.as_ref(),
            "contextmenu",
            Closure::wrap(Box::new(move |e: web_sys::Event| {
                e.prevent_default();
            }) as Box<dyn FnMut(_)>),
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn config() -> Config {
        Config::default()
    }

    #[test]
    fn every_action_names_at_least_one_code_and_the_table_is_the_sources_length() {
        assert_eq!(ACTIONS.len(), 17);
        for (action, codes) in ACTIONS {
            assert!(!codes.is_empty(), "{action:?} binds no code");
            assert_eq!(action.codes(), codes);
        }
    }

    #[test]
    fn a_press_is_an_edge_only_in_the_frame_it_resolved() {
        let mut input = Input::new();
        input.key_down("KeyW");
        // Nothing is visible until begin_frame resolves the queue.
        assert!(!input.held("KeyW"));
        input.begin_frame(&config(), None);
        assert!(input.held("KeyW"));
        assert!(input.pressed("KeyW"));
        // Next frame: still held, no longer an edge.
        input.begin_frame(&config(), None);
        assert!(input.held("KeyW"));
        assert!(!input.pressed("KeyW"));
        input.key_up("KeyW");
        input.begin_frame(&config(), None);
        assert!(!input.held("KeyW"));
        assert!(input.released("KeyW"));
    }

    #[test]
    fn a_down_and_up_inside_one_frame_still_registers_both_edges() {
        let mut input = Input::new();
        input.key_down("Space");
        input.key_up("Space");
        input.begin_frame(&config(), None);
        assert!(input.pressed("Space"), "the down was resolved first");
        assert!(input.released("Space"), "and the up right after it");
        assert!(!input.held("Space"));
    }

    #[test]
    fn actions_resolve_through_every_bound_code() {
        let mut input = Input::new();
        input.key_down("ArrowUp");
        input.key_down("KeyC");
        input.begin_frame(&config(), None);
        assert!(input.action(Action::Forward), "ArrowUp is bound to forward");
        assert!(input.action(Action::Crouch), "KeyC is bound to crouch");
        assert!(input.action_pressed(Action::Forward));
        assert!(!input.action(Action::Back));
    }

    #[test]
    fn blur_releases_every_held_key_so_the_player_cannot_run_forever() {
        let mut input = Input::new();
        input.key_down("KeyW");
        input.key_down("ShiftLeft");
        input.begin_frame(&config(), None);
        input.blur();
        input.begin_frame(&config(), None);
        assert!(!input.action(Action::Forward));
        assert!(!input.action(Action::Sprint));
        assert!(input.released("KeyW") && input.released("ShiftLeft"));
    }

    #[test]
    fn losing_pointer_lock_blurs() {
        let mut input = Input::new();
        input.pointer_locked = true;
        input.key_down("KeyD");
        input.begin_frame(&config(), None);
        input.set_pointer_locked(false);
        input.begin_frame(&config(), None);
        assert!(!input.action(Action::Right));
    }

    #[test]
    fn a_left_click_requests_the_lock_only_while_unlocked() {
        let mut input = Input::new();
        assert!(input.wants_pointer_lock(0));
        assert!(!input.wants_pointer_lock(2), "only button 0 asks");
        input.pointer_locked = true;
        assert!(!input.wants_pointer_lock(0));
    }

    #[test]
    fn mouse_look_accumulates_only_while_locked_and_scales_by_sensitivity() {
        let mut input = Input::new();
        input.mouse_move(10.0, 4.0);
        input.begin_frame(&config(), None);
        assert_eq!(
            input.look,
            Look { x: 0.0, y: 0.0 },
            "unlocked movement is ignored"
        );

        input.pointer_locked = true;
        input.mouse_move(10.0, 4.0);
        input.mouse_move(-2.0, 1.0);
        let cfg = config();
        input.begin_frame(&cfg, None);
        let s = f64::from(cfg.sensitivity);
        assert_eq!(input.look.x, 8.0 * s);
        assert_eq!(input.look.y, 5.0 * s);
        // The accumulator is consumed, not carried.
        input.begin_frame(&cfg, None);
        assert_eq!(input.look, Look { x: 0.0, y: 0.0 });
    }

    #[test]
    fn invert_y_flips_only_the_vertical_axis() {
        let mut input = Input::new();
        input.pointer_locked = true;
        input.mouse_move(3.0, 7.0);
        let mut cfg = config();
        cfg.invert_y = true;
        input.begin_frame(&cfg, None);
        let s = f64::from(cfg.sensitivity);
        assert_eq!(input.look.x, 3.0 * s);
        assert_eq!(input.look.y, -7.0 * s);
    }

    #[test]
    fn frozen_zeroes_the_look_delta_without_disturbing_keys() {
        let mut input = Input::new();
        input.pointer_locked = true;
        input.frozen = true;
        input.mouse_move(50.0, 50.0);
        input.key_down("KeyW");
        input.begin_frame(&config(), None);
        assert_eq!(input.look, Look { x: 0.0, y: 0.0 });
        assert!(input.action(Action::Forward));
    }

    #[test]
    fn disabled_input_queues_nothing() {
        let mut input = Input::new();
        input.enabled = false;
        input.key_down("KeyW");
        input.mouse_down(0);
        input.mouse_up(0);
        input.key_up("KeyW");
        input.wheel_event(-120.0);
        input.begin_frame(&config(), None);
        assert!(!input.action(Action::Forward));
        assert_eq!(input.wheel, 0.0);
    }

    #[test]
    fn the_wheel_latches_the_summed_sign_of_the_frames_events() {
        let mut input = Input::new();
        input.wheel_event(-120.0);
        input.wheel_event(-4.0);
        input.wheel_event(30.0);
        input.begin_frame(&config(), None);
        assert_eq!(input.wheel, -1.0, "sign(-120) + sign(-4) + sign(30)");
        input.begin_frame(&config(), None);
        assert_eq!(input.wheel, 0.0);
    }

    #[test]
    fn mouse_buttons_drive_fire_and_ads() {
        let mut input = Input::new();
        input.mouse_down(0);
        input.mouse_down(2);
        input.begin_frame(&config(), None);
        assert!(input.fire() && input.fire_pressed() && input.ads());
        input.begin_frame(&config(), None);
        assert!(input.fire() && !input.fire_pressed());
    }

    #[test]
    fn the_dead_zone_is_rescaled_not_clipped() {
        let mut input = Input::new();
        input.poll_gamepad(Some([0.15, -0.15, 0.0, 0.0]));
        assert_eq!(input.stick.move_x, 0.0);
        assert_eq!(input.stick.move_y, 0.0);
        input.poll_gamepad(Some([1.0, -1.0, 0.0, 0.0]));
        assert!(
            (input.stick.move_x - 1.0).abs() < 1e-12,
            "full deflection stays full"
        );
        assert!((input.stick.move_y + 1.0).abs() < 1e-12);
        input.poll_gamepad(Some([0.5, 0.0, 0.0, 0.0]));
        assert!((input.stick.move_x - (0.5 - 0.16) / 0.84).abs() < 1e-12);
    }

    #[test]
    fn the_look_curve_is_cubic_ish_and_sign_preserving() {
        let mut input = Input::new();
        input.poll_gamepad(Some([0.0, 0.0, 0.5, -0.5]));
        let dz = (0.5 - DEAD_ZONE) / (1.0 - DEAD_ZONE);
        let expected = dz.powf(LOOK_EXPONENT);
        assert!((input.stick.look_x - expected).abs() < 1e-12);
        assert!((input.stick.look_y + expected).abs() < 1e-12);
        // Well inside the dead zone the curve contributes nothing at all.
        input.poll_gamepad(Some([0.0, 0.0, 0.05, 0.0]));
        assert_eq!(input.stick.look_x, 0.0);
    }

    #[test]
    fn an_absent_pad_zeroes_every_stick_axis() {
        let mut input = Input::new();
        input.poll_gamepad(Some([1.0, 1.0, 1.0, 1.0]));
        input.poll_gamepad(None);
        assert_eq!(input.stick, Stick::default());
    }

    #[test]
    fn move_vector_clamps_a_diagonal_to_the_unit_disc() {
        let mut input = Input::new();
        input.key_down("KeyW");
        input.key_down("KeyD");
        input.begin_frame(&config(), None);
        let (x, y) = Input::move_vector(&input);
        assert!((x.hypot(y) - 1.0).abs() < 1e-12, "a diagonal is not faster");
        assert!((x - std::f64::consts::FRAC_1_SQRT_2).abs() < 1e-12);
        assert!((y - std::f64::consts::FRAC_1_SQRT_2).abs() < 1e-12);
    }

    #[test]
    fn move_vector_keeps_a_cardinal_at_exactly_one() {
        let mut input = Input::new();
        input.key_down("KeyS");
        input.begin_frame(&config(), None);
        assert_eq!(Input::move_vector(&input), (0.0, -1.0));
    }

    #[test]
    fn opposed_keys_cancel_and_the_stick_adds_with_an_inverted_y() {
        let mut input = Input::new();
        input.key_down("KeyA");
        input.key_down("KeyD");
        input.begin_frame(&config(), Some([0.0, -0.5, 0.0, 0.0]));
        let (x, y) = Input::move_vector(&input);
        assert_eq!(x, 0.0);
        // stick.moveY is subtracted, so a stick pushed forward (-Y) is +Y here.
        assert!(y > 0.0);
    }

    #[test]
    fn the_movement_seam_maps_every_input_action() {
        let mut input = Input::new();
        for code in [
            "Space",
            "KeyC",
            "KeyZ",
            "ShiftLeft",
            "KeyQ",
            "KeyE",
            "Mouse2",
        ] {
            input.key_down(code);
        }
        input.begin_frame(&config(), Some([0.0, 0.4, 0.0, 0.0]));
        let seam: &dyn PlayerInput = &input;
        assert!(seam.action(InputAction::Jump));
        assert!(seam.action(InputAction::Crouch));
        assert!(seam.action(InputAction::Prone));
        assert!(seam.action(InputAction::Sprint));
        assert!(seam.action(InputAction::LeanLeft));
        assert!(seam.action(InputAction::LeanRight));
        assert!(seam.ads(), "Mouse2 is ADS");
        assert!((seam.stick_move_y() - (0.4 - DEAD_ZONE) / (1.0 - DEAD_ZONE)).abs() < 1e-12);
        assert_eq!(seam.move_vector(), Input::move_vector(&input));
    }

    #[test]
    fn js_sign_returns_zero_for_zero_unlike_signum() {
        assert_eq!(js_sign(0.0), 0.0);
        assert_eq!(js_sign(-0.0), 0.0);
        assert_eq!(js_sign(-3.0), -1.0);
        assert_eq!(js_sign(3.0), 1.0);
    }

    #[test]
    fn end_frame_is_the_sources_no_op() {
        Input::new().end_frame();
    }
}
