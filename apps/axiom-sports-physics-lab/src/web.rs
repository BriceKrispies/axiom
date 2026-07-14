//! The `wasm32` live arm: decode browser input (keys, pointer-locked mouse
//! look, mouse buttons, wheel) into the lab's [`Intent`], drive the windowing
//! render loop, and paint the DOM HUD (center reticle, interaction label, and
//! the top-left debug line).
//!
//! Controls: click the canvas to capture the pointer; **mouse** looks;
//! **W/A/S/D** walk; **left click** picks up / tosses; **right click** drops
//! gently; **V or the wheel** zooms between first and third person; **R**
//! resets the lineup; **Esc** releases the pointer (browser-native).

use std::cell::RefCell;
use std::rc::Rc;

use axiom_windowing::WindowingApi;
use wasm_bindgen::prelude::*;
use wasm_bindgen::JsCast;
use web_sys::{KeyboardEvent, MouseEvent, WheelEvent};

use super::{live_app, CameraMode, Intent, LabHud, SportsPhysicsLab, CANVAS_ID, LIVE_CAPACITY};

/// Mouse-look sensitivity (radians per pixel of pointer movement).
const LOOK_SENSITIVITY: f32 = 0.0022;

/// Held key state + one-shot edges + accumulated mouse motion, drained into
/// each frame's [`Intent`].
#[derive(Default)]
struct Held {
    forward: bool,
    backward: bool,
    strafe_left: bool,
    strafe_right: bool,
    look_yaw: f32,
    look_pitch: f32,
    primary_edge: bool,
    secondary_edge: bool,
    toggle_edge: bool,
    zoom: f32,
    reset_edge: bool,
}

impl Held {
    fn intent(&mut self) -> Intent {
        Intent {
            forward: self.forward,
            backward: self.backward,
            strafe_left: self.strafe_left,
            strafe_right: self.strafe_right,
            look_yaw: std::mem::take(&mut self.look_yaw),
            look_pitch: std::mem::take(&mut self.look_pitch),
            primary: std::mem::take(&mut self.primary_edge),
            secondary: std::mem::take(&mut self.secondary_edge),
            toggle_view: std::mem::take(&mut self.toggle_edge),
            zoom: std::mem::take(&mut self.zoom),
            reset: std::mem::take(&mut self.reset_edge),
        }
    }
}

#[wasm_bindgen]
pub fn sports_physics_lab_start() {
    console_error_panic_hook::set_once();

    let held = Rc::new(RefCell::new(Held::default()));
    install_key_listeners(&held);
    install_mouse(&held);

    let mut windowing = WindowingApi::new();
    windowing
        .configure_surface(super::WIDTH, super::HEIGHT)
        .expect("surface dimensions are valid");

    let mut lab = SportsPhysicsLab::new();
    let (running, scene) = live_app(&mut lab);
    let meshes = running.mesh_set();
    let materials = running.material_textures();

    let hud = Hud::mount();
    let state = Rc::new(RefCell::new((lab, running, scene)));
    let frame_held = held.clone();
    let frame = move |tick: u64| {
        let mut guard = state.borrow_mut();
        let (lab, running, scene) = &mut *guard;
        let intent = frame_held.borrow_mut().intent();
        lab.step(intent);
        scene.update(running, lab);
        hud.update(&lab.hud());

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

/// Keydown / keyup listeners matched on the logical `key()` (so the gallery's
/// on-screen keypad's synthetic events drive the walk too).
fn install_key_listeners(held: &Rc<RefCell<Held>>) {
    let down = held.clone();
    let on_down = Closure::<dyn FnMut(KeyboardEvent)>::new(move |e: KeyboardEvent| {
        let mut h = down.borrow_mut();
        match e.key().as_str() {
            "w" | "W" | "ArrowUp" => {
                h.forward = true;
                e.prevent_default();
            }
            "s" | "S" | "ArrowDown" => {
                h.backward = true;
                e.prevent_default();
            }
            "a" | "A" | "ArrowLeft" => {
                h.strafe_left = true;
                e.prevent_default();
            }
            "d" | "D" | "ArrowRight" => {
                h.strafe_right = true;
                e.prevent_default();
            }
            "v" | "V" | "c" | "C" => h.toggle_edge = true,
            "r" | "R" => h.reset_edge = true,
            _ => {}
        }
    });
    let up = held.clone();
    let on_up = Closure::<dyn FnMut(KeyboardEvent)>::new(move |e: KeyboardEvent| {
        let mut h = up.borrow_mut();
        match e.key().as_str() {
            "w" | "W" | "ArrowUp" => h.forward = false,
            "s" | "S" | "ArrowDown" => h.backward = false,
            "a" | "A" | "ArrowLeft" => h.strafe_left = false,
            "d" | "D" | "ArrowRight" => h.strafe_right = false,
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

fn pointer_locked() -> bool {
    web_sys::window()
        .and_then(|w| w.document())
        .and_then(|d| d.pointer_lock_element())
        .is_some()
}

/// Pointer lock on canvas click, mouse-look while locked, button edges
/// (left = primary, right = secondary), and wheel zoom.
fn install_mouse(held: &Rc<RefCell<Held>>) {
    let window = web_sys::window().expect("a browser window");
    let document = window.document().expect("a document");
    let canvas = document
        .get_element_by_id(CANVAS_ID)
        .expect("the sports lab canvas element");

    // Click captures the pointer; a click while captured is the primary action.
    let lock_target = canvas.clone();
    let click_held = held.clone();
    let on_down = Closure::<dyn FnMut(MouseEvent)>::new(move |e: MouseEvent| {
        if !pointer_locked() {
            lock_target.request_pointer_lock();
            return;
        }
        let mut h = click_held.borrow_mut();
        match e.button() {
            0 => h.primary_edge = true,
            2 => h.secondary_edge = true,
            _ => {}
        }
    });
    canvas
        .add_event_listener_with_callback("mousedown", on_down.as_ref().unchecked_ref())
        .expect("mousedown listener installs");
    on_down.forget();

    // Suppress the context menu so right-click is a clean drop action.
    let on_context = Closure::<dyn FnMut(MouseEvent)>::new(move |e: MouseEvent| {
        e.prevent_default();
    });
    canvas
        .add_event_listener_with_callback("contextmenu", on_context.as_ref().unchecked_ref())
        .expect("contextmenu listener installs");
    on_context.forget();

    // Mouse-look (relative motion, only while locked). Mouse right → yaw+
    // (turn right); mouse up → pitch+ (look up).
    let move_held = held.clone();
    let on_move = Closure::<dyn FnMut(MouseEvent)>::new(move |e: MouseEvent| {
        if !pointer_locked() {
            return;
        }
        let mut h = move_held.borrow_mut();
        h.look_yaw += e.movement_x() as f32 * LOOK_SENSITIVITY;
        h.look_pitch -= e.movement_y() as f32 * LOOK_SENSITIVITY;
    });
    window
        .add_event_listener_with_callback("mousemove", on_move.as_ref().unchecked_ref())
        .expect("mousemove listener installs");
    on_move.forget();

    // Wheel: scroll down/back = zoom out (third person), up = zoom in.
    let wheel_held = held.clone();
    let on_wheel = Closure::<dyn FnMut(WheelEvent)>::new(move |e: WheelEvent| {
        e.prevent_default();
        let notches = (e.delta_y() as f32 / 100.0).clamp(-3.0, 3.0);
        wheel_held.borrow_mut().zoom += notches;
    });
    canvas
        .add_event_listener_with_callback("wheel", on_wheel.as_ref().unchecked_ref())
        .expect("wheel listener installs");
    on_wheel.forget();
}

/// The DOM HUD: a top-left debug line, a center reticle, and the interaction
/// label under it. Mounted once over the canvas.
struct Hud {
    debug: web_sys::Element,
    label: web_sys::Element,
}

impl Hud {
    fn mount() -> Hud {
        let document = web_sys::window()
            .and_then(|w| w.document())
            .expect("a document");
        let body = document.body().expect("a body");

        let make = |style: &str| {
            let el = document.create_element("div").expect("hud element");
            el.set_attribute("style", style).ok();
            body.append_child(&el).ok();
            el
        };

        let debug = make(
            "position:fixed;top:10px;left:12px;z-index:20;color:#f2f6ff;\
             font:14px/1.5 ui-monospace,Menlo,Consolas,monospace;\
             text-shadow:0 1px 3px #000;pointer-events:none;white-space:pre;",
        );
        let reticle = make(
            "position:fixed;top:50%;left:50%;transform:translate(-50%,-50%);\
             z-index:20;color:#ffffff;font:700 20px/1 ui-monospace,monospace;\
             text-shadow:0 0 4px #000;pointer-events:none;",
        );
        reticle.set_text_content(Some("+"));
        let label = make(
            "position:fixed;top:54%;left:50%;transform:translate(-50%,0);\
             z-index:20;color:#eaffea;font:15px/1.3 ui-monospace,monospace;\
             text-shadow:0 1px 3px #000;pointer-events:none;",
        );
        Hud { debug, label }
    }

    fn update(&self, hud: &LabHud) {
        let mode = match hud.mode {
            CameraMode::FirstPerson => "first-person",
            CameraMode::ThirdPerson => "third-person",
        };
        self.debug.set_text_content(Some(&format!(
            "SPORTS PHYSICS LAB\nheld: {}   camera: {}   physics step: {}",
            hud.held.unwrap_or("—"),
            mode,
            hud.physics_step,
        )));
        let label = match (hud.held, hud.hover) {
            (Some(_), _) => "click: Toss   ·   right-click: Drop".to_string(),
            (None, Some(name)) => format!("click: Pick up {name}"),
            (None, None) => String::new(),
        };
        self.label.set_text_content(Some(&label));
    }
}
