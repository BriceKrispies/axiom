//! The `wasm32` live arm: drive the windowing render loop, stepping the physics
//! world and re-authoring the scene every frame so the crucible's bodies fall,
//! bounce, and pile **live** on the gallery canvas. Never compiled on native — the
//! deterministic physics + scene authoring live in the other modules; this is the
//! thin browser edge (the windowing loop + keyboard/keypad input).
//!
//! Controls (real keyboard or the gallery's on-screen keypad, which dispatches
//! synthetic key events): **▲ / Space / K** kick every dynamic body upward so the
//! pile scatters and re-settles; **R** resets the room and re-drops it. The camera
//! slowly orbits on its own.

use std::cell::RefCell;
use std::rc::Rc;

use axiom::prelude::Vec3;
use axiom_windowing::WindowingApi;
use wasm_bindgen::prelude::*;
use wasm_bindgen::JsCast;
use web_sys::KeyboardEvent;

use crate::physics_crucible::all_stations;
use crate::physics_crucible::crucible_camera::orbit;
use crate::physics_crucible::crucible_scenario::{KindTag, Station};
use crate::physics_crucible::physics_crucible_app::{
    author_live, live_app, live_instances, live_surface_size, CrucibleWorld, CANVAS_ID,
    LIVE_CAPACITY,
};

/// How many steps a drop runs before the room resets and re-drops (a watchable
/// rhythm: drop → settle → brief rest → re-drop).
const LIVE_LOOP_STEPS: u64 = 420;

/// Shared one-shot input flags, set by the key listeners and drained each frame.
#[derive(Default)]
struct Input {
    kick: bool,
    reset: bool,
}

/// The live driver: the physics world, its stations, and the loop step counter.
struct LiveCrucible {
    world: CrucibleWorld,
    stations: Vec<Box<dyn Station>>,
    step: u64,
}

impl LiveCrucible {
    fn new() -> Self {
        let stations = all_stations();
        let mut world = CrucibleWorld::new();
        for station in &stations {
            station.populate(&mut world);
        }
        LiveCrucible {
            world,
            stations,
            step: 0,
        }
    }

    /// Tear the room down and re-drop it from the start.
    fn reset(&mut self) {
        self.world = CrucibleWorld::new();
        for station in &self.stations {
            station.populate(&mut self.world);
        }
        self.step = 0;
    }

    /// Kick every dynamic body upward (with a little per-body lateral spread, keyed
    /// off the deterministic handle) so the pile scatters and re-settles.
    fn kick(&mut self) {
        let handles: Vec<_> = self
            .world
            .bodies()
            .iter()
            .filter(|b| b.kind == KindTag::Dynamic)
            .map(|b| (b.handle, b.handle.raw()))
            .collect();
        for (handle, raw) in handles {
            let lateral = ((raw % 5) as f32 - 2.0) * 1.2;
            let lateral_z = ((raw % 3) as f32 - 1.0) * 1.2;
            self.world
                .apply_impulse(handle, Vec3::new(lateral, 7.0, lateral_z));
        }
    }

    /// Advance one step, applying any pending input first, and loop at the end.
    fn advance(&mut self, input: &mut Input) {
        if std::mem::take(&mut input.reset) || self.step >= LIVE_LOOP_STEPS {
            self.reset();
        }
        if std::mem::take(&mut input.kick) {
            self.kick();
        }
        let n = self.step;
        for station in &self.stations {
            station.script(&mut self.world, n);
        }
        self.world.step(n);
        self.step += 1;
    }
}

/// Browser entry: build the live room, capture keyboard/keypad input, and drive
/// the windowing render loop — stepping physics and re-authoring the scene each
/// frame so the simulation plays out on screen.
#[wasm_bindgen]
pub fn physics_start() {
    console_error_panic_hook::set_once();

    let input = Rc::new(RefCell::new(Input::default()));
    install_key_listener(&input);

    let (width, height) = live_surface_size();
    let mut windowing = WindowingApi::new();
    windowing
        .configure_surface(width, height)
        .expect("surface dimensions are valid");

    let live = LiveCrucible::new();
    let (eye, target) = orbit(0);
    let running = live_app(live_instances(&live.world), eye, target);
    let meshes = running.mesh_set();
    let materials = running.material_textures();

    let state = Rc::new(RefCell::new((live, running)));
    let frame_input = input.clone();
    let frame = move |tick: u64| {
        let mut guard = state.borrow_mut();
        let (live, running) = &mut *guard;
        live.advance(&mut frame_input.borrow_mut());
        let instances = live_instances(&live.world);
        let (eye, target) = orbit(live.step);
        running.reauthor(move |world, meshes, materials| {
            author_live(world, meshes, materials, &instances, eye, target)
        });
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
            // The frame's SDF raymarch scene (the floating sphere), composited over
            // the meshes by the live backend — GPU in the browser, Canvas2D in the
            // software fallback (and where browser WebGPU is unavailable).
            outcome.sdf_scene().cloned(),
        )
    };

    let _ = windowing.run_web_multi(CANVAS_ID, meshes, materials, LIVE_CAPACITY, frame);
}

/// Install a `keydown` listener that maps the demo's keys/keypad into one-shot
/// input flags. Matches on the logical `key()` so the gallery's synthetic-keyboard
/// on-screen pad drives it too.
fn install_key_listener(input: &Rc<RefCell<Input>>) {
    let input = input.clone();
    let callback = Closure::<dyn FnMut(KeyboardEvent)>::new(move |e: KeyboardEvent| {
        let mut flags = input.borrow_mut();
        match e.key().as_str() {
            "ArrowUp" | " " | "k" | "K" => {
                flags.kick = true;
                e.prevent_default();
            }
            "r" | "R" | "ArrowDown" => flags.reset = true,
            _ => {}
        }
    });
    web_sys::window()
        .expect("a browser window")
        .add_event_listener_with_callback("keydown", callback.as_ref().unchecked_ref())
        .expect("key listener installs");
    callback.forget();
}
