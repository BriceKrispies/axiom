//! The live `wasm32` arm: keyboard capture, the windowing render loop, and the
//! DOM HUD. Never compiled on native — the deterministic game core lives in
//! `lib.rs`; this is the thin nondeterministic edge.
//!
//! Tank controls (so the gallery's synthetic-key on-screen pad drives it on
//! touch): ↑/W forward, ↓/S back, ←/→ turn, A/D strafe, Space fire.

use std::cell::RefCell;
use std::rc::Rc;

use axiom_windowing::WindowingApi;
use wasm_bindgen::prelude::*;
use wasm_bindgen::JsCast;
use web_sys::{Element, KeyboardEvent};

use super::{build_retro_fps_app, RetroFpsGame, Intent, CANVAS_ID};

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
        }
    }
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
            let intent = keys.borrow().intent();
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

/// The DOM heads-up display: a stats bar and a centre crosshair, overlaid on the
/// page. Text rendering is not an engine concern, so the HUD lives in the DOM
/// the app owns — updated each frame from the deterministic [`super::Hud`].
struct Hud {
    bar: Element,
}

impl Hud {
    fn mount() -> Hud {
        let document = web_sys::window()
            .expect("a browser window")
            .document()
            .expect("a document");
        let body = document.body().expect("a document body");

        let bar = document.create_element("div").expect("create div");
        bar.set_attribute(
            "style",
            "position:fixed;left:50%;top:10px;transform:translateX(-50%);\
             z-index:10;pointer-events:none;font:600 15px ui-monospace,monospace;\
             color:#e8ecf2;background:rgba(10,12,16,0.65);padding:6px 14px;\
             border-radius:8px;white-space:nowrap;",
        )
        .expect("style bar");
        body.append_child(&bar).expect("append bar");

        let crosshair = document.create_element("div").expect("create div");
        crosshair
            .set_attribute(
                "style",
                "position:fixed;left:50%;top:50%;transform:translate(-50%,-50%);\
                 z-index:10;pointer-events:none;font:700 22px ui-monospace,monospace;\
                 color:rgba(255,255,255,0.75);",
            )
            .expect("style crosshair");
        crosshair.set_text_content(Some("+"));
        body.append_child(&crosshair).expect("append crosshair");

        Hud { bar }
    }

    fn update(&self, hud: &super::Hud) {
        self.bar.set_text_content(Some(&format!(
            "HP {:>3}   SCORE {:>5}   AMMO {:>3}   ENEMIES {}",
            hud.health.max(0),
            hud.score,
            hud.ammo,
            hud.enemies_alive,
        )));
    }
}
