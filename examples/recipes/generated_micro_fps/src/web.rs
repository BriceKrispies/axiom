//! The browser (wasm32) entry. Expands the level, then drives the engine's live
//! windowing present loop — the same `run_web_multi` path `App::run` uses, but
//! with the scene authored *post-build* and `max_instances` set to the real
//! generated renderable count (so the live backend sizes its instance buffers to
//! the generated content). Force the software renderer with `?backend=canvas2d`
//! (the sandbox has no browser WebGPU).
//!
//! Controls are cloned from the gallery's first-person `forest_walk` demo:
//! **WASD** move (arrow up/down also move), **mouse** looks once you click the
//! canvas to capture the pointer (classic FPS pointer-lock), and arrow-left/right
//! turn. Held keys are polled each frame; mouse deltas accumulate between frames
//! and drain each tick — so a tap turns once and stops (no runaway spin).
//!
//! Two entries: [`micro_fps_start`] is the navigable first person;
//! [`micro_fps_overview_start`] pins a fixed overview camera.

#![cfg(target_arch = "wasm32")]

use std::cell::RefCell;
use std::rc::Rc;

use axiom::prelude::{Angle, FirstPersonInput, Vec3};
use axiom_windowing::WindowingApi;
use wasm_bindgen::prelude::*;
use wasm_bindgen::JsCast;

use crate::scenes::{expand_level, expand_level_view, ExpandedLevel};
use crate::style::Style;

const W: u32 = 960;
const H: u32 = 600;
/// Meters moved per frame while a movement key is held.
const MOVE_SPEED: f32 = 0.22;
/// Radians turned per frame while an arrow-turn key is held.
const TURN_SPEED: f32 = 0.028;
/// Radians of look per pixel of mouse movement.
const LOOK_SENS: f32 = 0.0022;

/// Held movement/turn keys, polled each frame.
#[derive(Clone, Copy, Default)]
struct Keys {
    forward: bool,
    backward: bool,
    strafe_left: bool,
    strafe_right: bool,
    turn_left: bool,
    turn_right: bool,
}

/// Mouse-look deltas accumulated between frames (radians), drained each tick.
/// The engine controller treats yaw/pitch as **per-frame deltas** and integrates
/// them itself, so this is never self-accumulated here (doing so is what caused
/// the runaway drift).
#[derive(Clone, Copy, Default)]
struct Look {
    yaw: f32,
    pitch: f32,
}

fn document() -> web_sys::Document {
    web_sys::window().and_then(|w| w.document()).expect("a document")
}

fn pointer_is_locked() -> bool {
    document().pointer_lock_element().is_some()
}

/// Install a keydown/keyup listener that sets the held-keys state (`e.code()`
/// physical keys, so layout-independent).
fn install_key_listener(keys: &Rc<RefCell<Keys>>, event: &str, pressed: bool) {
    let keys = keys.clone();
    let cb = Closure::<dyn FnMut(web_sys::KeyboardEvent)>::new(move |e: web_sys::KeyboardEvent| {
        let mut k = keys.borrow_mut();
        match e.code().as_str() {
            "KeyW" | "ArrowUp" => k.forward = pressed,
            "KeyS" | "ArrowDown" => k.backward = pressed,
            "KeyA" => k.strafe_left = pressed,
            "KeyD" => k.strafe_right = pressed,
            "ArrowLeft" => k.turn_left = pressed,
            "ArrowRight" => k.turn_right = pressed,
            _ => {}
        }
    });
    let _ = document().add_event_listener_with_callback(event, cb.as_ref().unchecked_ref());
    cb.forget();
}

/// Capture the pointer when the canvas is clicked (classic FPS mouse-look).
fn install_pointer_lock(canvas_id: &str) {
    if let Some(canvas) = document().get_element_by_id(canvas_id) {
        let target = canvas.clone();
        let cb = Closure::<dyn FnMut()>::new(move || {
            target.unchecked_ref::<web_sys::HtmlElement>().request_pointer_lock();
        });
        let _ = canvas.add_event_listener_with_callback("click", cb.as_ref().unchecked_ref());
        cb.forget();
    }
}

/// Accumulate relative mouse movement into yaw/pitch while the pointer is locked.
fn install_mouse_look(look: &Rc<RefCell<Look>>) {
    let look = look.clone();
    let cb = Closure::<dyn FnMut(web_sys::MouseEvent)>::new(move |e: web_sys::MouseEvent| {
        if pointer_is_locked() {
            let mut l = look.borrow_mut();
            // Mouse right (movement_x > 0) turns the view right, which is a
            // *negative* yaw delta (engine yaw is CCW about +Y). Mouse up
            // (movement_y < 0) looks up.
            l.yaw -= e.movement_x() as f32 * LOOK_SENS;
            l.pitch -= e.movement_y() as f32 * LOOK_SENS;
        }
    });
    let _ = document().add_event_listener_with_callback("mousemove", cb.as_ref().unchecked_ref());
    cb.forget();
}

/// Drive `expanded` through the live present loop. When `interactive`, install the
/// FPS controls and feed the engine's first-person controller each frame.
fn present(canvas_id: &str, mut expanded: ExpandedLevel, interactive: bool) {
    console_error_panic_hook::set_once();
    let meshes = expanded.app.mesh_set();
    let materials = expanded.app.material_textures();
    let max_instances = expanded.renderable_count as u32;
    let mut running = expanded.app;

    let keys = Rc::new(RefCell::new(Keys::default()));
    let look = Rc::new(RefCell::new(Look::default()));
    if interactive {
        install_key_listener(&keys, "keydown", true);
        install_key_listener(&keys, "keyup", false);
        install_pointer_lock(canvas_id);
        install_mouse_look(&look);
    }

    let mut windowing = WindowingApi::new();
    windowing.configure_surface(W, H).expect("surface dimensions valid");
    let _ = windowing.run_web_multi(canvas_id, meshes, materials, max_instances, move |tick| {
        if interactive {
            // Drain this frame's mouse-look delta and poll the held keys, then feed
            // the engine controller PER-FRAME DELTAS (it integrates + clamps pitch
            // itself). When the mouse stops and no keys are held, the delta is zero,
            // so the view holds still — no drift.
            let l = {
                let mut look = look.borrow_mut();
                let drained = *look;
                *look = Look::default();
                drained
            };
            let k = *keys.borrow();
            let yaw_delta = (k.turn_left as i32 - k.turn_right as i32) as f32 * TURN_SPEED + l.yaw;
            let pitch_delta = l.pitch;
            // Local-space move: +X strafes right, -Z walks forward.
            let strafe = (k.strafe_right as i32 - k.strafe_left as i32) as f32 * MOVE_SPEED;
            let forward = (k.forward as i32 - k.backward as i32) as f32 * MOVE_SPEED;
            let move_local = Vec3::new(strafe, 0.0, -forward);
            running.control(FirstPersonInput::new(0, move_local, Angle::radians(yaw_delta), Angle::radians(pitch_delta)));
        }
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
    });
}

/// Boot the navigable first-person facility on `canvas_id`. WASD moves, click to
/// capture the mouse for looking (arrow-left/right also turn).
#[wasm_bindgen]
pub fn micro_fps_start(canvas_id: &str) {
    present(canvas_id, expand_level(&Style::facility()), true);
}

/// Boot the fixed overview camera (the whole facility in one frame) on
/// `canvas_id`.
#[wasm_bindgen]
pub fn micro_fps_overview_start(canvas_id: &str) {
    let style = Style::facility();
    let expanded = expand_level_view(&style, Vec3::new(15.0, 17.0, 18.0), Vec3::new(0.0, 0.0, -18.0));
    present(canvas_id, expanded, false);
}
