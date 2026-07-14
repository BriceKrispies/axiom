//! The `wasm32` live arm: capture keyboard/keypad input, drive the windowing
//! render loop (stepping the game and refreshing the ball + camera each frame via
//! the persistent meshed scene), and paint a small DOM HUD.
//!
//! Controls: **the mouse** aims the camera (click the canvas to capture the
//! pointer); **W/S** drive the ball toward / away from the camera and **A/D** strafe
//! it left / right relative to the camera; **Shift** brakes; braked and nearly
//! stopped, **tapping** a move key charges a spin and **releasing Shift** launches
//! it; **R** restarts. The gallery's on-screen keypad dispatches synthetic key
//! events, so the buttons drive movement too.

use std::cell::RefCell;
use std::rc::Rc;

use axiom_windowing::WindowingApi;
use wasm_bindgen::prelude::*;
use wasm_bindgen::JsCast;
use web_sys::{KeyboardEvent, MouseEvent};

use crate::{live_app, GravixGame, Intent, Phase, SpinState, CANVAS_ID, LIVE_CAPACITY};

/// Held key state + accumulated mouse motion, drained into each frame's `Intent`.
#[derive(Default)]
struct Held {
    forward: bool,
    back: bool,
    left: bool,
    right: bool,
    brake: bool,
    tap_edge: bool,
    restart_edge: bool,
    /// Mouse motion accumulated since the last frame (pixels), for camera orbit.
    mouse_dx: f32,
    mouse_dy: f32,
}

impl Held {
    /// The `Intent` for this frame, consuming the one-shot edges + mouse motion.
    fn intent(&mut self) -> Intent {
        Intent {
            forward: self.forward,
            back: self.back,
            left: self.left,
            right: self.right,
            brake: self.brake,
            tap: std::mem::take(&mut self.tap_edge),
            // Mouse right → orbit right; mouse up → orbit up (invert screen Y).
            yaw_delta: std::mem::take(&mut self.mouse_dx),
            pitch_delta: -std::mem::take(&mut self.mouse_dy),
            restart: std::mem::take(&mut self.restart_edge),
        }
    }
}

#[wasm_bindgen]
pub fn gravix_start() {
    console_error_panic_hook::set_once();

    let held = Rc::new(RefCell::new(Held::default()));
    install_key_listeners(&held);
    install_mouse_look(&held);

    let mut windowing = WindowingApi::new();
    windowing.configure_surface(1280, 720).expect("surface dimensions are valid");

    let game = GravixGame::new();
    let (running, scene) = live_app(&game);
    let meshes = running.mesh_set();
    let materials = running.material_textures();

    let state = Rc::new(RefCell::new((game, running, scene)));
    let frame_held = held.clone();
    let frame = move |tick: u64| {
        let mut guard = state.borrow_mut();
        let (game, running, scene) = &mut *guard;
        let intent = frame_held.borrow_mut().intent();
        game.step(intent);
        game.render(running, scene);
        update_hud(game);

        let outcome = running.tick(tick);
        let lights = outcome
            .lights()
            .iter()
            .map(|l| (l.kind(), l.vec(), l.color(), l.intensity()))
            .collect();
        (
            outcome.clear_color(),
            lights,
            outcome.light_view_proj(),
            outcome.mesh_batches(),
            outcome.camera_view_proj(),
            outcome.mesh_batch_casters(),
            outcome.sdf_scene().cloned(),
        )
    };

    let _ = windowing.run_web_multi(CANVAS_ID, meshes, materials, LIVE_CAPACITY, frame);
}

/// Install keydown / keyup listeners on the window, matching logical `key()` so
/// the gallery's synthetic-keyboard keypad drives it too. A move key's *first*
/// press (rising edge, not auto-repeat) sets `tap_edge` for the spin charge.
fn install_key_listeners(held: &Rc<RefCell<Held>>) {
    let down = held.clone();
    let on_down = Closure::<dyn FnMut(KeyboardEvent)>::new(move |e: KeyboardEvent| {
        let mut h = down.borrow_mut();
        match e.key().as_str() {
            "w" | "W" | "ArrowUp" => {
                h.tap_edge |= !h.forward;
                h.forward = true;
                e.prevent_default();
            }
            "s" | "S" | "ArrowDown" => {
                h.tap_edge |= !h.back;
                h.back = true;
                e.prevent_default();
            }
            "a" | "A" | "ArrowLeft" => {
                h.tap_edge |= !h.left;
                h.left = true;
                e.prevent_default();
            }
            "d" | "D" | "ArrowRight" => {
                h.tap_edge |= !h.right;
                h.right = true;
                e.prevent_default();
            }
            "Shift" => h.brake = true,
            "r" | "R" => h.restart_edge = true,
            _ => {}
        }
    });
    let up = held.clone();
    let on_up = Closure::<dyn FnMut(KeyboardEvent)>::new(move |e: KeyboardEvent| {
        let mut h = up.borrow_mut();
        match e.key().as_str() {
            "w" | "W" | "ArrowUp" => h.forward = false,
            "s" | "S" | "ArrowDown" => h.back = false,
            "a" | "A" | "ArrowLeft" => h.left = false,
            "d" | "D" | "ArrowRight" => h.right = false,
            "Shift" => h.brake = false,
            _ => {}
        }
    });
    let window = web_sys::window().expect("a browser window");
    window
        .add_event_listener_with_callback("keydown", on_down.as_ref().unchecked_ref())
        .expect("keydown listener installs");
    window
        .add_event_listener_with_callback("keyup", on_up.as_ref().unchecked_ref())
        .expect("keyup listener installs");
    on_down.forget();
    on_up.forget();
}

/// Capture the pointer on canvas click and accumulate relative mouse motion into
/// the camera-orbit deltas while it is locked — so the mouse (and nothing else)
/// aims the camera.
fn install_mouse_look(held: &Rc<RefCell<Held>>) {
    let window = web_sys::window().expect("a browser window");
    let document = window.document().expect("a document");
    let canvas = document.get_element_by_id(CANVAS_ID).expect("the gravix canvas element");

    // Click the canvas to capture the pointer.
    let lock_target = canvas.clone();
    let on_click = Closure::<dyn FnMut(MouseEvent)>::new(move |_e: MouseEvent| {
        let _ = lock_target.request_pointer_lock();
    });
    canvas
        .add_event_listener_with_callback("click", on_click.as_ref().unchecked_ref())
        .expect("click listener installs");
    on_click.forget();

    // Accumulate relative motion only while the pointer is locked.
    let move_held = held.clone();
    let on_move = Closure::<dyn FnMut(MouseEvent)>::new(move |e: MouseEvent| {
        let locked = web_sys::window()
            .and_then(|w| w.document())
            .and_then(|d| d.pointer_lock_element())
            .is_some();
        if !locked {
            return;
        }
        let mut h = move_held.borrow_mut();
        h.mouse_dx += e.movement_x() as f32;
        h.mouse_dy += e.movement_y() as f32;
    });
    window
        .add_event_listener_with_callback("mousemove", on_move.as_ref().unchecked_ref())
        .expect("mousemove listener installs");
    on_move.forget();
}

/// Create (once) and update the DOM HUD overlay with the run state.
fn update_hud(game: &GravixGame) {
    let document = match web_sys::window().and_then(|w| w.document()) {
        Some(d) => d,
        None => return,
    };
    let hud = match document.get_element_by_id("gravix-hud") {
        Some(el) => el,
        None => {
            let el = document.create_element("div").expect("create hud div");
            el.set_id("gravix-hud");
            el.set_attribute(
                "style",
                "position:fixed;top:10px;left:12px;z-index:20;color:#f6e9ff;\
                 font:16px/1.4 ui-monospace,Menlo,Consolas,monospace;\
                 text-shadow:0 0 6px #b040ff,0 1px 2px #000;pointer-events:none;",
            )
            .ok();
            if let Some(body) = document.body() {
                body.append_child(&el).ok();
            }
            el
        }
    };
    let banner = match game.phase() {
        Phase::Rolling => match game.spin_state() {
            SpinState::Braking => "  —  BRAKING".to_string(),
            SpinState::SpinCharging => "  —  CHARGING ⚡ (release to launch)".to_string(),
            _ => String::new(),
        },
        Phase::Finished => "  —  FINISH! · press R".to_string(),
        Phase::FellOut => "  —  OUT! resetting…".to_string(),
    };
    hud.set_text_content(Some(&format!("GRAVIX   SPEED {:>3.0}{}", game.speed(), banner)));
}
