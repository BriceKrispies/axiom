//! Touch controls for a phone-shaped screen.
//!
//! **This has no counterpart in the source.** `apps/shmup/src/core/input.js`
//! handles keyboard, mouse and gamepad and nothing else, so every number and
//! every gesture below is INVENTED — there is no `input.js:NNN` to cite and none
//! is claimed. The layout constants are the only ones in this crate chosen by
//! feel rather than transcribed, and they are gathered at the top of the file so
//! that stays obvious.
//!
//! # Why this is a translation layer and not a control scheme
//!
//! Nothing here reaches gameplay. Touch produces exactly what a gamepad already
//! produces — a `[move_x, move_y, look_x, look_y]` axis quad — plus the same
//! `Mouse0`/`Mouse2` codes a mouse produces, and hands both to [`crate::input`].
//! The dead zone, the look response curve, the fire gate, the ADS blend and the
//! movement state machine are then the ones the port already has, unchanged and
//! unforked.
//!
//! That is the whole design. A second control path would have meant a second
//! place for "what does aiming feel like" to live, and the two would drift; the
//! source's own `_pollGamepad` is the precedent for feeding synthetic axes into
//! one shared snapshot rather than special-casing a device downstream.
//!
//! # The gestures
//!
//! * **Left half — a thumbstick that appears where you touch it.** The base is
//!   placed at the touch-down point rather than pinned to a fixed corner, so the
//!   stick is always under the thumb that reached for it. Deflection is clamped
//!   to [`STICK_RADIUS_PX`] and normalised, which is what makes it an axis rather
//!   than a swipe.
//! * **Right half — drag to look.** Fed as pointer DELTAS through
//!   [`crate::input::Input::mouse_move`], the same path a mouse takes, because a
//!   drag should turn the view by an amount rather than hold a turn rate. Using
//!   the pad's look axes instead would have made a stationary finger keep
//!   turning.
//! * **Right half — tap to aim.** A press that ends within [`TAP_MS`] and moves
//!   less than [`TAP_SLOP_PX`] toggles ADS. The slop is what separates it from a
//!   short look-drag, and both thresholds have to be generous: a thumb on glass
//!   is not a mouse click.
//! * **Fire button.** Its own element, so it never steals a look-drag.
//!
//! ADS is a TOGGLE here, not a hold. On a mouse the right button is held while
//! aiming; a thumb cannot hold one corner and still look with the same hand.

#![cfg(target_arch = "wasm32")]

use std::cell::RefCell;
use std::rc::Rc;

use wasm_bindgen::prelude::*;
use wasm_bindgen::JsCast;

use crate::input::dom::SharedInput;

// ---------------------------------------------------------------------------
// INVENTED CONSTANTS. Every one, without exception - see the module doc.
// ---------------------------------------------------------------------------

/// How far the thumb may travel from the stick's origin before the axis reads
/// full deflection. Roughly a thumb's comfortable arc on a phone.
const STICK_RADIUS_PX: f64 = 68.0;

/// Longest press still counted as a tap rather than the beginning of a drag.
const TAP_MS: f64 = 240.0;

/// How far a press may wander and still be a tap. A thumb on glass moves several
/// pixels even when the person meant to hold still, so this cannot be small.
const TAP_SLOP_PX: f64 = 14.0;

/// Screen width at or below which the touch overlay is offered at all.
const MOBILE_MAX_WIDTH_PX: f64 = 900.0;

/// Look sensitivity, in mouse-pixels per touch-pixel.
///
/// ABOVE 1.0, which is the opposite of the reasoning that first set it to 0.55
/// ("a thumb drag is shorter than a mouse sweep"). That argument has the sign
/// backwards: a shorter available stroke is exactly why each pixel of it has to
/// buy MORE rotation. Measured on a 414 px-wide phone, 0.55 turned a full
/// screen-width swipe through 28 degrees - 6.4 swipes to turn around, which is
/// unusable in a shooter. At 2.10 one swipe is a little over a quarter turn, so
/// an about-face is under two.
const LOOK_GAIN: f64 = 2.10;

/// The overlay's stylesheet. Sits above the canvas and below the HUD, and is
/// `pointer-events: none` except on the two controls, so a look-drag that
/// crosses the joystick's ring is not swallowed by it.
const CSS: &str = "\
#ax-touch{position:fixed;inset:0;z-index:40;pointer-events:none;\
 touch-action:none;-webkit-user-select:none;user-select:none}\
#ax-touch .stick{position:absolute;width:136px;height:136px;margin:-68px 0 0 -68px;\
 border-radius:50%;border:2px solid rgba(255,255,255,.22);\
 background:radial-gradient(circle,rgba(255,255,255,.10),rgba(255,255,255,.02));\
 opacity:0;transition:opacity .12s ease-out}\
#ax-touch .stick.live{opacity:1}\
#ax-touch .knob{position:absolute;width:58px;height:58px;margin:-29px 0 0 -29px;\
 border-radius:50%;background:rgba(255,255,255,.30);\
 border:2px solid rgba(255,255,255,.45);opacity:0;transition:opacity .12s ease-out}\
#ax-touch .knob.live{opacity:1}\
#ax-touch .fire{position:absolute;right:22px;bottom:118px;width:92px;height:92px;\
 border-radius:50%;pointer-events:auto;\
 border:2px solid rgba(255,255,255,.40);background:rgba(255,90,60,.22);\
 display:flex;align-items:center;justify-content:center;\
 font:600 13px/1 ui-monospace,Menlo,Consolas,monospace;letter-spacing:.14em;\
 color:rgba(255,255,255,.82)}\
#ax-touch .fire.down{background:rgba(255,90,60,.45);transform:scale(.94)}\
#ax-touch .ads{position:absolute;right:30px;bottom:230px;padding:9px 14px;\
 border-radius:15px;border:1px solid rgba(255,255,255,.28);\
 background:rgba(0,0,0,.30);\
 font:600 11px/1 ui-monospace,Menlo,Consolas,monospace;letter-spacing:.12em;\
 color:rgba(255,255,255,.55)}\
#ax-touch .ads.on{background:rgba(120,190,255,.30);color:rgba(255,255,255,.95);\
 border-color:rgba(160,210,255,.65)}";

/// Whether this looks like a phone or tablet rather than a desktop that happens
/// to have a touchscreen.
///
/// Both halves are load-bearing. `pointer: coarse` alone would put a thumbstick
/// on a touchscreen laptop being driven with a mouse; a width test alone would
/// put one on a small desktop window. Requiring both means the overlay appears
/// when the primary input really is a finger on a small screen.
pub fn is_mobile() -> bool {
    let Some(window) = web_sys::window() else {
        return false;
    };
    let coarse = window
        .match_media("(pointer: coarse)")
        .ok()
        .flatten()
        .is_some_and(|m| m.matches());
    let narrow = window
        .inner_width()
        .ok()
        .and_then(|v| v.as_f64())
        .is_some_and(|w| w <= MOBILE_MAX_WIDTH_PX);
    coarse && narrow
}

/// One pointer's worth of look-drag bookkeeping, kept so a release can decide
/// whether the gesture was a tap.
#[derive(Debug, Clone, Copy)]
struct LookDrag {
    id: i32,
    last_x: f64,
    last_y: f64,
    start_ms: f64,
    /// PATH length, not the straight-line offset from where the press began.
    /// A thumb that wanders out and comes back has still been dragged, and
    /// storing the origin instead would have called that a tap.
    travelled: f64,
}

/// The overlay's live state. The DOM listeners write it; the frame loop reads
/// [`TouchControls::pad`].
#[derive(Debug, Default)]
struct State {
    /// The joystick pointer, and where its base was placed.
    stick: Option<(i32, f64, f64)>,
    move_x: f64,
    move_y: f64,
    look: Option<LookDrag>,
    ads: bool,
}

/// The handle the frame loop keeps.
#[derive(Clone)]
pub struct TouchControls {
    state: Rc<RefCell<State>>,
}

impl TouchControls {
    /// The synthetic gamepad quad, or `None` when the stick is idle.
    ///
    /// `move_y` is negated because [`crate::input::Input::move_vector`]
    /// SUBTRACTS the stick's Y (`y -= self.stick.move_y`), following the gamepad
    /// convention that forward is negative. Screen-space "up" is negative Y too,
    /// so the two cancel and this returns the value a real pad would report.
    ///
    /// The look axes are always zero: look is fed as deltas through
    /// `mouse_move`, not as a held axis.
    pub fn pad(&self) -> Option<[f64; 4]> {
        let s = self.state.borrow();
        let idle = s.move_x == 0.0 && s.move_y == 0.0;
        (!idle).then_some([s.move_x, s.move_y, 0.0, 0.0])
    }
}

fn now_ms() -> f64 {
    web_sys::window()
        .and_then(|w| w.performance())
        .map_or(0.0, |p| p.now())
}

fn el(document: &web_sys::Document, tag: &str, class: &str) -> web_sys::HtmlElement {
    let e: web_sys::HtmlElement = document
        .create_element(tag)
        .expect("the document creates an element")
        .unchecked_into();
    e.set_class_name(class);
    e
}

fn put(e: &web_sys::HtmlElement, x: f64, y: f64) {
    let _ = e
        .style()
        .set_property("transform", &format!("translate({x}px,{y}px)"));
}

fn toggle(e: &web_sys::HtmlElement, class: &str, on: bool) {
    // `classList.toggle(token, force)` - the DOM's own conditional, so there is
    // no branch here and no chance of running both sides.
    //
    // This was `[list.remove_1(c), list.add_1(c)][usize::from(on)]`, which is
    // WRONG for effects: an array literal evaluates every element before the
    // index picks one, so `add` ran on every call and nothing could ever turn
    // off. Table selection chooses a VALUE; it cannot choose whether an effect
    // happens.
    let _ = e.class_list().toggle_with_force(class, on);
}

/// Build the overlay and wire it to `input`. Returns the handle whose
/// [`TouchControls::pad`] the frame loop folds in beside the real gamepad.
///
/// Call only when [`is_mobile`] says so: on a desktop this would put a
/// thumbstick over the game and swallow the pointer.
pub fn attach(input: &SharedInput) -> TouchControls {
    let window = web_sys::window().expect("a browser window");
    let document = window.document().expect("a document");
    let body = document.body().expect("a body");

    let style = document
        .create_element("style")
        .expect("the document creates a style element");
    style.set_text_content(Some(CSS));
    let _ = document
        .head()
        .expect("a document head")
        .append_child(&style);

    let root = el(&document, "div", "");
    root.set_id("ax-touch");
    let stick = el(&document, "div", "stick");
    let knob = el(&document, "div", "knob");
    let fire = el(&document, "div", "fire");
    fire.set_text_content(Some("FIRE"));
    let ads = el(&document, "div", "ads");
    ads.set_text_content(Some("ADS"));
    let _ = root.append_child(&stick);
    let _ = root.append_child(&knob);
    let _ = root.append_child(&fire);
    let _ = root.append_child(&ads);
    let _ = body.append_child(&root);

    // `Input::mouse_move` discards deltas unless the pointer is locked - the
    // source's guard against the view swinging while the cursor is loose in the
    // page (`input.js:120`). A touchscreen has no Pointer Lock API to satisfy it
    // and needs none: a drag IS the capture, because it cannot begin anywhere
    // but on the surface and it ends when the finger lifts.
    //
    // So the flag is set once, here. It is not a lie about the browser state -
    // everything downstream reads it as "pointer movement drives the view", and
    // on a phone that is true for exactly as long as the overlay is attached.
    // The alternative, a second `touch_look` gate beside it, would have forked
    // the one condition the look path turns on.
    input.borrow_mut().set_pointer_locked(true);

    let state = Rc::new(RefCell::new(State::default()));
    let controls = TouchControls { state: Rc::clone(&state) };

    // ---- fire button -----------------------------------------------------
    // Its own element with `pointer-events: auto`, so a press here is never
    // mistaken for the beginning of a look-drag.
    {
        let input = Rc::clone(input);
        let btn = fire.clone();
        let down = Closure::<dyn FnMut(web_sys::PointerEvent)>::new(
            move |e: web_sys::PointerEvent| {
                e.prevent_default();
                // The button sits inside the overlay, which sits inside the body
                // the field handlers are bound to. Without this the press also
                // reaches the field as the start of a look-drag on the right half,
                // and its release then reads as a tap and toggles ADS - so firing
                // would silently zoom.
                e.stop_propagation();
                input.borrow_mut().mouse_down(0);
                toggle(&btn, "down", true);
            },
        );
        let _ = fire.add_event_listener_with_callback(
            "pointerdown",
            down.as_ref().unchecked_ref(),
        );
        down.forget();
    }
    {
        let input = Rc::clone(input);
        let btn = fire.clone();
        let up = Closure::<dyn FnMut(web_sys::PointerEvent)>::new(
            move |e: web_sys::PointerEvent| {
                e.prevent_default();
                e.stop_propagation();
                input.borrow_mut().mouse_up(0);
                toggle(&btn, "down", false);
            },
        );
        for name in ["pointerup", "pointercancel", "pointerleave"] {
            let _ = fire
                .add_event_listener_with_callback(name, up.as_ref().unchecked_ref());
        }
        up.forget();
    }

    // ---- the field: joystick on the left, look on the right --------------
    let half = move || {
        web_sys::window()
            .and_then(|w| w.inner_width().ok())
            .and_then(|v| v.as_f64())
            .unwrap_or(MOBILE_MAX_WIDTH_PX)
            / 2.0
    };

    {
        let state = Rc::clone(&state);
        let (stick_e, knob_e) = (stick.clone(), knob.clone());
        let down = Closure::<dyn FnMut(web_sys::PointerEvent)>::new(
            move |e: web_sys::PointerEvent| {
                e.prevent_default();
                let (x, y) = (f64::from(e.client_x()), f64::from(e.client_y()));
                let mut s = state.borrow_mut();
                let left = x < half();
                // Left half claims the stick unless one is already down; the
                // right half starts a look-drag that may still turn out to be a
                // tap.
                let free = s.stick.is_none();
                (left && free).then(|| {
                    s.stick = Some((e.pointer_id(), x, y));
                    put(&stick_e, x, y);
                    put(&knob_e, x, y);
                    toggle(&stick_e, "live", true);
                    toggle(&knob_e, "live", true);
                });
                (!left).then(|| {
                    s.look = Some(LookDrag {
                        id: e.pointer_id(),
                        last_x: x,
                        last_y: y,
                        start_ms: now_ms(),
                        travelled: 0.0,
                    });
                });
            },
        );
        let _ = body.add_event_listener_with_callback(
            "pointerdown",
            down.as_ref().unchecked_ref(),
        );
        down.forget();
    }

    {
        let state = Rc::clone(&state);
        let input = Rc::clone(input);
        let knob_e = knob.clone();
        let mv = Closure::<dyn FnMut(web_sys::PointerEvent)>::new(
            move |e: web_sys::PointerEvent| {
                let (x, y) = (f64::from(e.client_x()), f64::from(e.client_y()));
                let id = e.pointer_id();
                let mut s = state.borrow_mut();

                // Joystick: clamp the offset to the ring and normalise it.
                let stick_hit = s.stick.filter(|(sid, _, _)| *sid == id);
                stick_hit.map(|(_, ox, oy)| {
                    let (dx, dy) = (x - ox, y - oy);
                    let len = dx.hypot(dy);
                    let clamped = len.min(STICK_RADIUS_PX);
                    let scale = clamped / len.max(1e-6);
                    put(&knob_e, ox + dx * scale, oy + dy * scale);
                    s.move_x = dx / STICK_RADIUS_PX.max(1e-6);
                    s.move_y = dy / STICK_RADIUS_PX.max(1e-6);
                    let mag = s.move_x.hypot(s.move_y);
                    let over = mag.max(1.0);
                    s.move_x /= over;
                    s.move_y /= over;
                });

                // Look: feed the delta down the mouse path.
                let look_hit = s.look.filter(|l| l.id == id);
                look_hit.map(|mut l| {
                    let (dx, dy) = (x - l.last_x, y - l.last_y);
                    l.travelled += dx.hypot(dy);
                    l.last_x = x;
                    l.last_y = y;
                    s.look = Some(l);
                    input
                        .borrow_mut()
                        .mouse_move(dx * LOOK_GAIN, dy * LOOK_GAIN);
                });
            },
        );
        let _ = body
            .add_event_listener_with_callback("pointermove", mv.as_ref().unchecked_ref());
        mv.forget();
    }

    {
        let state = Rc::clone(&state);
        let input = Rc::clone(input);
        let (stick_e, knob_e, ads_e) = (stick.clone(), knob.clone(), ads.clone());
        let up = Closure::<dyn FnMut(web_sys::PointerEvent)>::new(
            move |e: web_sys::PointerEvent| {
                let id = e.pointer_id();
                let mut s = state.borrow_mut();

                let released_stick = s.stick.is_some_and(|(sid, _, _)| sid == id);
                released_stick.then(|| {
                    s.stick = None;
                    s.move_x = 0.0;
                    s.move_y = 0.0;
                    toggle(&stick_e, "live", false);
                    toggle(&knob_e, "live", false);
                });

                let l = s.look.filter(|l| l.id == id);
                l.map(|l| {
                    s.look = None;
                    // A tap - short and nearly still - toggles ADS. `travelled`
                    // is the path length rather than the straight-line offset, so
                    // a wobble that returns to where it started still counts as a
                    // drag, which is what a thumb actually does.
                    let quick = now_ms() - l.start_ms <= TAP_MS;
                    let still = l.travelled <= TAP_SLOP_PX;
                    (quick && still).then(|| {
                        s.ads = !s.ads;
                        let on = s.ads;
                        toggle(&ads_e, "on", on);
                        // `Mouse2` is what `Input::ads` reads, so the toggle
                        // reaches the same gate a right mouse button does.
                        let mut i = input.borrow_mut();
                        // Same trap as `toggle` above: an array literal would
                        // have run BOTH, leaving the button down forever.
                        on.then(|| i.mouse_down(2));
                        (!on).then(|| i.mouse_up(2));
                    });
                });
            },
        );
        for name in ["pointerup", "pointercancel"] {
            let _ = body
                .add_event_listener_with_callback(name, up.as_ref().unchecked_ref());
        }
        up.forget();
    }

    controls
}
