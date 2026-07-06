//! The `wasm32` live arm: capture keyboard/keypad input, drive the windowing
//! render loop (stepping the game and re-authoring the scene each frame), and
//! paint a small DOM HUD.
//!
//! Controls: **W A S D** roll the marble (camera-relative), **Space** jumps,
//! **Shift** brakes, the **arrow keys** orbit the camera, and **R** restarts the
//! run once it is over. The gallery's on-screen keypad dispatches synthetic key
//! events, so the WASD / jump buttons drive it too.

use std::cell::RefCell;
use std::rc::Rc;

use axiom_windowing::WindowingApi;
use wasm_bindgen::prelude::*;
use wasm_bindgen::JsCast;
use web_sys::KeyboardEvent;

use crate::gravix::{author_scene, live_app, GravixGame, Intent, Phase, CANVAS_ID, LIVE_CAPACITY};

/// Held key state, plus one-shot edges drained into each frame's `Intent`.
#[derive(Default)]
struct Held {
    forward: bool,
    back: bool,
    left: bool,
    right: bool,
    brake: bool,
    yaw_left: bool,
    yaw_right: bool,
    pitch_up: bool,
    pitch_down: bool,
    jump_edge: bool,
    restart_edge: bool,
}

impl Held {
    /// The `Intent` for this frame, consuming the one-shot edges.
    fn intent(&mut self) -> Intent {
        let jump = std::mem::take(&mut self.jump_edge);
        let restart = std::mem::take(&mut self.restart_edge);
        Intent {
            forward: self.forward,
            back: self.back,
            left: self.left,
            right: self.right,
            brake: self.brake,
            jump,
            yaw_left: self.yaw_left,
            yaw_right: self.yaw_right,
            pitch_up: self.pitch_up,
            pitch_down: self.pitch_down,
            restart,
        }
    }
}

#[wasm_bindgen]
pub fn gravix_start() {
    console_error_panic_hook::set_once();

    let held = Rc::new(RefCell::new(Held::default()));
    install_key_listeners(&held);

    let mut windowing = WindowingApi::new();
    windowing
        .configure_surface(1280, 720)
        .expect("surface dimensions are valid");

    let game = GravixGame::new();
    let running = live_app(&game);
    let meshes = running.mesh_set();
    let materials = running.material_textures();

    let state = Rc::new(RefCell::new((game, running)));
    let frame_held = held.clone();
    let frame = move |tick: u64| {
        let mut guard = state.borrow_mut();
        let (game, running) = &mut *guard;
        let intent = frame_held.borrow_mut().intent();
        game.step(intent);

        let instances = game.render_instances();
        let (eye, target) = game.camera();
        running.reauthor(move |world, meshes, materials| {
            author_scene(world, meshes, materials, &instances, eye, target)
        });
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
/// the gallery's synthetic-keyboard keypad drives it too.
fn install_key_listeners(held: &Rc<RefCell<Held>>) {
    let down = held.clone();
    let on_down = Closure::<dyn FnMut(KeyboardEvent)>::new(move |e: KeyboardEvent| {
        let mut h = down.borrow_mut();
        match e.key().as_str() {
            "w" | "W" => h.forward = true,
            "s" | "S" => h.back = true,
            "a" | "A" => h.left = true,
            "d" | "D" => h.right = true,
            "Shift" => h.brake = true,
            " " => {
                h.jump_edge = true;
                e.prevent_default();
            }
            "ArrowLeft" => {
                h.yaw_left = true;
                e.prevent_default();
            }
            "ArrowRight" => {
                h.yaw_right = true;
                e.prevent_default();
            }
            "ArrowUp" => {
                h.pitch_up = true;
                e.prevent_default();
            }
            "ArrowDown" => {
                h.pitch_down = true;
                e.prevent_default();
            }
            "r" | "R" => h.restart_edge = true,
            _ => {}
        }
    });
    let up = held.clone();
    let on_up = Closure::<dyn FnMut(KeyboardEvent)>::new(move |e: KeyboardEvent| {
        let mut h = up.borrow_mut();
        match e.key().as_str() {
            "w" | "W" => h.forward = false,
            "s" | "S" => h.back = false,
            "a" | "A" => h.left = false,
            "d" | "D" => h.right = false,
            "Shift" => h.brake = false,
            "ArrowLeft" => h.yaw_left = false,
            "ArrowRight" => h.yaw_right = false,
            "ArrowUp" => h.pitch_up = false,
            "ArrowDown" => h.pitch_down = false,
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
        Phase::Playing => String::new(),
        Phase::LevelComplete => "  —  LEVEL CLEAR".to_string(),
        Phase::Dead => "  —  OUT!".to_string(),
        Phase::RunOver => "  —  RUN OVER · press R".to_string(),
    };
    hud.set_text_content(Some(&format!(
        "GRAVIX   LEVEL {}   COINS {}/{}   RUN {}   FALLS {}/{}{}",
        game.level_number(),
        game.coins_collected(),
        game.coins_total(),
        game.run_score(),
        game.falls(),
        crate::gravix::settings::RUN_MAX_FALLS,
        banner,
    )));
}
