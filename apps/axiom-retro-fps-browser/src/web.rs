//! The live `wasm32` arm: keyboard + mouse-look capture, the windowing render
//! loop, and the DOM HUD. Never compiled on native — the deterministic game core
//! lives in `lib.rs`; this is the thin nondeterministic edge.
//!
//! Controls. Desktop: click the canvas to capture the mouse (Pointer Lock), then
//! mouse to look (yaw + pitch), WASD to move, left-click to fire, Esc to release.
//! Touch/keyboard: the gallery's synthetic-key on-screen pad — ←/→ turn, ↑/↓
//! move, FIRE — and arrows/WASD/Space — all still work alongside the mouse.

use std::cell::RefCell;
use std::rc::Rc;

use axiom_windowing::WindowingApi;
use wasm_bindgen::prelude::*;
use wasm_bindgen::JsCast;
use web_sys::{Element, KeyboardEvent, MouseEvent};

use super::{build_retro_fps_app, RetroFpsGame, Intent, CANVAS_ID};

/// Radians of look per pixel of mouse movement.
const MOUSE_SENSITIVITY: f32 = 0.0025;

/// Held-key state, polled into an [`Intent`] each frame.
#[derive(Default, Clone, Copy)]
struct Keys {
    forward: bool,
    backward: bool,
    turn_left: bool,
    turn_right: bool,
    strafe_left: bool,
    strafe_right: bool,
    fire: bool,
}

impl Keys {
    fn intent(self) -> Intent {
        Intent {
            forward: self.forward,
            backward: self.backward,
            turn_left: self.turn_left,
            turn_right: self.turn_right,
            strafe_left: self.strafe_left,
            strafe_right: self.strafe_right,
            fire: self.fire,
            look_yaw: 0.0,
            look_pitch: 0.0,
        }
    }
}

/// Mouse-look deltas accumulated between frames (radians), drained each tick.
#[derive(Default, Clone, Copy)]
struct Look {
    yaw: f32,
    pitch: f32,
}

/// Log a line to the browser console, prefixed so the demo is easy to spot.
fn log(msg: &str) {
    web_sys::console::log_1(&JsValue::from_str(&format!("[retro_fps] {msg}")));
}

/// The browser entry: build the game + engine app, capture the keyboard, mount
/// the HUD, and drive the live windowing loop.
#[wasm_bindgen]
pub fn start() {
    console_error_panic_hook::set_once();
    log("start(): building level");

    let keys = Rc::new(RefCell::new(Keys::default()));
    install_key_listener(&keys, "keydown", true);
    install_key_listener(&keys, "keyup", false);

    // Mouse-look: click captures the pointer; movement accumulates yaw/pitch;
    // left-click fires while captured.
    let look = Rc::new(RefCell::new(Look::default()));
    install_pointer_lock();
    install_mouse_look(&look);
    install_mouse_fire(&keys);

    let hud = Hud::mount();

    let mut game = RetroFpsGame::new();
    let mut running = build_retro_fps_app();
    let (vertices, indices) = running.mesh_vertex_stream();
    let max_instances = running.renderable_count() as u32;

    let mut windowing = WindowingApi::new();
    windowing
        .configure_surface(960, 600)
        .expect("surface dimensions are valid");

    let mut tick: u64 = 0;
    let _ = windowing.run_web(
        CANVAS_ID,
        vertices,
        indices,
        max_instances,
        move |_raf_tick| {
            // Fold this frame's accumulated mouse-look into the held-key intent,
            // then reset the accumulator.
            let mut intent = keys.borrow().intent();
            {
                let mut l = look.borrow_mut();
                intent.look_yaw = l.yaw;
                intent.look_pitch = l.pitch;
                *l = Look::default();
            }
            let commands = game.step(intent);
            let outcome = running.tick_with_controls(tick, &commands.enemies, &[commands.control]);
            tick += 1;
            hud.update(&commands.hud);
            (
                outcome.clear_color(),
                outcome.instance_floats(),
                outcome.draws().len() as u32,
            )
        },
    );
}

/// Map a key's pressed state into the shared key set. Matches on `key` (not
/// `code`) so the gallery's synthetic-keyboard on-screen pad drives it too.
fn install_key_listener(keys: &Rc<RefCell<Keys>>, event: &str, pressed: bool) {
    let keys = keys.clone();
    let callback = Closure::<dyn FnMut(KeyboardEvent)>::new(move |e: KeyboardEvent| {
        let mut k = keys.borrow_mut();
        match e.key().as_str() {
            "ArrowUp" | "w" | "W" => k.forward = pressed,
            "ArrowDown" | "s" | "S" => k.backward = pressed,
            "ArrowLeft" => k.turn_left = pressed,
            "ArrowRight" => k.turn_right = pressed,
            "a" | "A" => k.strafe_left = pressed,
            "d" | "D" => k.strafe_right = pressed,
            " " => k.fire = pressed,
            _ => return,
        }
        // Stop the browser from scrolling on the arrow keys / space.
        e.prevent_default();
    });
    web_sys::window()
        .expect("a browser window")
        .add_event_listener_with_callback(event, callback.as_ref().unchecked_ref())
        .expect("key listener installs");
    callback.forget();
}

/// The presentation canvas element.
fn retro_fps_canvas() -> Element {
    web_sys::window()
        .expect("a browser window")
        .document()
        .expect("a document")
        .get_element_by_id(CANVAS_ID)
        .expect("the retro_fps canvas is in the page")
}

/// Is the pointer currently locked (to our canvas)?
fn pointer_is_locked() -> bool {
    web_sys::window()
        .and_then(|w| w.document())
        .and_then(|d| d.pointer_lock_element())
        .is_some()
}

/// Capture the pointer when the canvas is clicked (classic FPS mouse-look).
fn install_pointer_lock() {
    let canvas = retro_fps_canvas();
    let target = canvas.clone();
    let cb = Closure::<dyn FnMut(MouseEvent)>::new(move |_e: MouseEvent| {
        let _ = target.request_pointer_lock();
    });
    canvas
        .add_event_listener_with_callback("click", cb.as_ref().unchecked_ref())
        .expect("click listener installs");
    cb.forget();
}

/// Accumulate relative mouse movement into yaw/pitch while the pointer is locked.
/// Mouse right turns right (−yaw); mouse up looks up (+pitch).
fn install_mouse_look(look: &Rc<RefCell<Look>>) {
    let look = look.clone();
    let cb = Closure::<dyn FnMut(MouseEvent)>::new(move |e: MouseEvent| {
        if !pointer_is_locked() {
            return;
        }
        let mut l = look.borrow_mut();
        l.yaw += -(e.movement_x() as f32) * MOUSE_SENSITIVITY;
        l.pitch += -(e.movement_y() as f32) * MOUSE_SENSITIVITY;
    });
    web_sys::window()
        .expect("a browser window")
        .add_event_listener_with_callback("mousemove", cb.as_ref().unchecked_ref())
        .expect("mousemove listener installs");
    cb.forget();
}

/// Left mouse button fires while the pointer is locked (release always clears,
/// so a button held across an unlock can't stick). The first, lock-engaging
/// click happens before lock, so it does not fire.
fn install_mouse_fire(keys: &Rc<RefCell<Keys>>) {
    let window = web_sys::window().expect("a browser window");
    let down_keys = keys.clone();
    let down = Closure::<dyn FnMut(MouseEvent)>::new(move |e: MouseEvent| {
        if e.button() == 0 && pointer_is_locked() {
            down_keys.borrow_mut().fire = true;
        }
    });
    let up_keys = keys.clone();
    let up = Closure::<dyn FnMut(MouseEvent)>::new(move |e: MouseEvent| {
        if e.button() == 0 {
            up_keys.borrow_mut().fire = false;
        }
    });
    window
        .add_event_listener_with_callback("mousedown", down.as_ref().unchecked_ref())
        .expect("mousedown listener installs");
    window
        .add_event_listener_with_callback("mouseup", up.as_ref().unchecked_ref())
        .expect("mouseup listener installs");
    down.forget();
    up.forget();
}

/// The DOM heads-up display: a stats bar and a centre crosshair, overlaid on the
/// page. Text rendering is not an engine concern, so the HUD lives in the DOM
/// the app owns — updated each frame from the deterministic [`super::Hud`].
struct Hud {
    bar: Element,
    hint: Element,
}

impl Hud {
    fn mount() -> Hud {
        let document = web_sys::window()
            .expect("a browser window")
            .document()
            .expect("a document");

        // Anchor the overlay to the CANVAS, not the viewport: wrap the canvas in
        // a position:relative box and make the canvas fill it, so the crosshair
        // and HUD bar (position:absolute children) are centred on the canvas and
        // scroll with it — instead of being pinned to the viewport centre. The
        // wrapper also owns the responsive size (overriding the host page's
        // canvas CSS), which keeps the 960x600 (8:5) aspect undistorted.
        let canvas = document
            .get_element_by_id(CANVAS_ID)
            .expect("retro_fps canvas is in the page");
        let parent = canvas.parent_node().expect("canvas has a parent");
        let wrap = document.create_element("div").expect("create div");
        wrap.set_attribute(
            "style",
            "position:relative;display:block;width:100%;max-width:960px;\
             margin:0 auto;line-height:0;",
        )
        .expect("style wrap");
        // Put the wrapper where the canvas was, then move the canvas inside it.
        parent
            .insert_before(&wrap, Some(&canvas))
            .expect("insert wrapper");
        wrap.append_child(&canvas).expect("reparent canvas");
        canvas
            .set_attribute(
                "style",
                "display:block;width:100%;height:auto;max-width:100%;\
                 aspect-ratio:8/5;border:1px solid #2a2e36;border-radius:8px;\
                 background:#000;touch-action:none;",
            )
            .expect("style canvas");

        let bar = document.create_element("div").expect("create div");
        bar.set_attribute(
            "style",
            "position:absolute;top:8px;left:50%;transform:translateX(-50%);\
             z-index:10;pointer-events:none;font:600 15px ui-monospace,monospace;\
             color:#e8ecf2;background:rgba(10,12,16,0.65);padding:6px 14px;\
             border-radius:8px;white-space:nowrap;",
        )
        .expect("style bar");
        wrap.append_child(&bar).expect("append bar");

        let crosshair = document.create_element("div").expect("create div");
        crosshair
            .set_attribute(
                "style",
                "position:absolute;left:50%;top:50%;transform:translate(-50%,-50%);\
                 z-index:10;pointer-events:none;font:700 22px ui-monospace,monospace;\
                 color:rgba(255,255,255,0.8);",
            )
            .expect("style crosshair");
        crosshair.set_text_content(Some("+"));
        wrap.append_child(&crosshair).expect("append crosshair");

        // A discoverability hint, hidden once the pointer is captured.
        let hint = document.create_element("div").expect("create div");
        hint.set_attribute(
            "style",
            "position:absolute;bottom:10px;left:50%;transform:translateX(-50%);\
             z-index:10;pointer-events:none;font:500 13px ui-monospace,monospace;\
             color:#cdd3dc;background:rgba(10,12,16,0.6);padding:5px 12px;\
             border-radius:8px;white-space:nowrap;",
        )
        .expect("style hint");
        hint.set_text_content(Some("Click to look · WASD move · click to fire · Esc to release"));
        wrap.append_child(&hint).expect("append hint");

        Hud { bar, hint }
    }

    fn update(&self, hud: &super::Hud) {
        self.bar.set_text_content(Some(&format!(
            "HP {:>3}   SCORE {:>5}   AMMO {:>3}   ENEMIES {}",
            hud.health.max(0),
            hud.score,
            hud.ammo,
            hud.enemies_alive,
        )));
        // Hide the hint once the mouse is captured; show it again when released.
        // (The `hidden` attribute hides whenever present, so toggle presence.)
        if pointer_is_locked() {
            let _ = self.hint.set_attribute("hidden", "");
        } else {
            let _ = self.hint.remove_attribute("hidden");
        }
    }
}
