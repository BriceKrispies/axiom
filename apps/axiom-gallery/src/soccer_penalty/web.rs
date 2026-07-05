//! The soccer-penalty gallery arm: the `wasm32` browser entry
//! (`soccer_penalty_start`) that drives the windowing run loop, steps the session
//! from keyboard/pad input, and re-authors the scene every frame.
//!
//! Rendering goes through [`crate::soccer_penalty::penalty_render_meshed`] — the
//! **same shared meshed scene the headless convergence champion uses** — so the
//! gallery and the champion can never diverge. The scene registers its low-poly
//! mesh library once, then updates each frame with runtime `spawn`/`despawn`; the
//! render plan's flat, pre-shaded colours (Pass 3) ride as each material's base
//! colour, which every backend (WebGPU / WebGL2 / Canvas2D) feeds into its
//! per-instance colour.

#[cfg(target_arch = "wasm32")]
use std::cell::RefCell;
#[cfg(target_arch = "wasm32")]
use std::rc::Rc;
#[cfg(target_arch = "wasm32")]
use axiom_windowing::WindowingApi;
#[cfg(target_arch = "wasm32")]
use wasm_bindgen::prelude::*;
#[cfg(target_arch = "wasm32")]
use wasm_bindgen::JsCast;
#[cfg(target_arch = "wasm32")]
use web_sys::KeyboardEvent;

// The live gallery renders through the SAME shared meshed scene the headless
// convergence champion uses, so the two can never diverge.
#[cfg(target_arch = "wasm32")]
use crate::soccer_penalty::penalty_render_meshed::{soccer_meshed_shell, PenaltyMeshedScene};
// The world-item count check in tests reads the render plan's content tag.
#[cfg(test)]
use crate::soccer_penalty::penalty_render_plan::PenaltyRenderContent;
// Only the wasm entry + native tests build a session.
#[cfg(any(test, target_arch = "wasm32"))]
use crate::soccer_penalty::SoccerPenaltyApp;

#[cfg(target_arch = "wasm32")]
const CANVAS_ID: &str = "axiom-soccer-penalty-canvas";
#[cfg(target_arch = "wasm32")]
const WIDTH: u32 = 960;
#[cfg(target_arch = "wasm32")]
const HEIGHT: u32 = 600;
/// The live instance cap: the diorama draws ~180 objects, well under this.
#[cfg(target_arch = "wasm32")]
const CAPACITY: u32 = 1024;

// --- wasm32 browser arm -----------------------------------------------------

/// One-shot + held keyboard state, drained into a `PenaltyInputIntent` per tick.
#[cfg(target_arch = "wasm32")]
#[derive(Default)]
struct SoccerInput {
    aim_x: i32,
    aim_y: i32,
    charge_held: bool,
    release_edge: bool,
    continue_edge: bool,
    reset_edge: bool,
}

#[cfg(target_arch = "wasm32")]
impl SoccerInput {
    fn drain(&mut self) -> crate::soccer_penalty::PenaltyInputIntent {
        use crate::soccer_penalty::PenaltyInputIntent as In;
        let intent = if core::mem::take(&mut self.reset_edge) {
            In::resetting()
        } else if core::mem::take(&mut self.continue_edge) {
            In::continuing()
        } else if core::mem::take(&mut self.release_edge) {
            In::releasing()
        } else if self.charge_held {
            In::charging(self.aim_x, self.aim_y)
        } else if self.aim_x != 0 || self.aim_y != 0 {
            In::aiming(self.aim_x, self.aim_y)
        } else {
            In::neutral()
        };
        intent
    }
}

/// Browser entry: drive the windowing run loop, stepping the session from
/// keyboard/pad input and re-authoring the diorama each frame.
#[cfg(target_arch = "wasm32")]
#[wasm_bindgen]
pub fn soccer_penalty_start() {
    console_error_panic_hook::set_once();

    let input = Rc::new(RefCell::new(SoccerInput::default()));
    install_key_listener(&input);

    let mut windowing = WindowingApi::new();
    windowing
        .configure_surface(WIDTH, HEIGHT)
        .expect("surface dimensions are valid");

    // Build the shared meshed scene once (registering the mesh library + a stable
    // material palette before the live backend snapshots them), author the start
    // frame, then re-author it each frame with runtime spawn/despawn — the exact
    // scene the convergence champion renders.
    let session = SoccerPenaltyApp::new_session();
    let initial = SoccerPenaltyApp::build_session_frame(&session);
    let mut running = soccer_meshed_shell();
    let mut scene = PenaltyMeshedScene::install(&mut running);
    scene.set_view(&mut running, initial.render_plan.camera);
    scene.author(&mut running, &initial);
    let meshes = running.mesh_set();
    let materials = running.material_textures();

    let state = Rc::new(RefCell::new((session, running, scene)));
    let frame_input = input.clone();
    let frame = move |tick: u64| {
        let mut guard = state.borrow_mut();
        let (session, running, scene) = &mut *guard;
        let raw = frame_input.borrow_mut().drain();
        // Between rounds the SHOOT/Space press doubles as "continue", so the
        // 5-button on-screen pad (4 arrows + SHOOT) can play the whole loop.
        let waiting = matches!(
            session.loop_state,
            crate::soccer_penalty::penalty_session::PenaltyLoopState::BetweenRounds
                | crate::soccer_penalty::penalty_session::PenaltyLoopState::RoundAwarded
                | crate::soccer_penalty::penalty_session::PenaltyLoopState::SessionComplete
        );
        let intent = if waiting && (raw.charge_pressed || raw.release_pressed) {
            crate::soccer_penalty::PenaltyInputIntent::continuing()
        } else {
            raw
        };
        *session = session.clone().advance(intent);
        let diorama = SoccerPenaltyApp::build_session_frame(session);
        scene.author(running, &diorama);
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

    let _ = windowing.run_web_multi(CANVAS_ID, meshes, materials, CAPACITY, frame);
}

/// Match on the logical `key()` so the gallery's synthetic on-screen keypad
/// drives it too (arrows aim, Space charges/shoots, R resets, Enter continues).
#[cfg(target_arch = "wasm32")]
fn install_key_listener(input: &Rc<RefCell<SoccerInput>>) {
    let down = input.clone();
    let on_down = Closure::<dyn FnMut(KeyboardEvent)>::new(move |e: KeyboardEvent| {
        let mut f = down.borrow_mut();
        match e.key().as_str() {
            "ArrowLeft" | "a" | "A" => {
                f.aim_x = -100;
                e.prevent_default();
            }
            "ArrowRight" | "d" | "D" => {
                f.aim_x = 100;
                e.prevent_default();
            }
            "ArrowUp" | "w" | "W" => {
                f.aim_y = 100;
                e.prevent_default();
            }
            "ArrowDown" | "s" | "S" => {
                f.aim_y = -100;
                e.prevent_default();
            }
            " " | "k" | "K" => {
                f.charge_held = true;
                e.prevent_default();
            }
            "r" | "R" => f.reset_edge = true,
            "Enter" => f.continue_edge = true,
            _ => {}
        }
    });
    let up = input.clone();
    let on_up = Closure::<dyn FnMut(KeyboardEvent)>::new(move |e: KeyboardEvent| {
        let mut f = up.borrow_mut();
        match e.key().as_str() {
            "ArrowLeft" | "a" | "A" | "ArrowRight" | "d" | "D" => f.aim_x = 0,
            "ArrowUp" | "w" | "W" | "ArrowDown" | "s" | "S" => f.aim_y = 0,
            " " | "k" | "K" => {
                // Releasing the charge fires the shot exactly once.
                let was_charging = core::mem::take(&mut f.charge_held);
                f.release_edge = was_charging;
            }
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn authors_the_diorama_into_the_engine_scene() {
        use crate::soccer_penalty::penalty_render_meshed::soccer_meshed_app;
        use crate::soccer_penalty::penalty_scene::DioramaRole;
        let frame = SoccerPenaltyApp::build_stage1();
        // Every non-athlete world object is a static draw; the athletes (kicker +
        // goalie) are one continuous SKINNED body per kit material, submitted as
        // skinned draws (deformed by a joint palette), not static draws.
        let is_athlete = |r| matches!(r, DioramaRole::Kicker | DioramaRole::Goalie);
        let mut non_athlete = 0usize;
        let mut kit_materials = std::collections::BTreeSet::new();
        frame.render_plan.items.iter().for_each(|it| {
            if let PenaltyRenderContent::World { role, material, .. } = it.content {
                if is_athlete(role) {
                    kit_materials.insert(material);
                } else {
                    non_athlete += 1;
                }
            }
        });
        let mut app = soccer_meshed_app(frame);
        let outcome = app.tick(0);
        // Non-athletes draw statically; the athletes are one skinned body per kit material.
        assert_eq!(outcome.draws().len(), non_athlete);
        assert_eq!(outcome.skinned_draws().len(), kit_materials.len());
        assert!(kit_materials.len() < 31, "athletes drew as {} grouped skinned bodies", kit_materials.len());
    }

    /// Regression guard: the athletes are one **skinned** body per kit material,
    /// baked ONCE at the bind pose and deformed each frame by a joint palette —
    /// never re-baked. Re-baking `MetaSurface` per frame dropped the game to ~7 FPS
    /// and leaked a fresh mesh per group per frame; and because the live
    /// `run_web_multi` uploads meshes only at bind, those per-frame meshes never
    /// reached the GPU (the athletes vanished). This asserts the bind bake happens
    /// once and the mesh store never grows afterward.
    #[test]
    fn live_loop_skins_bodies_once_and_never_rebakes() {
        use crate::soccer_penalty::penalty_render_meshed::{soccer_meshed_shell, PenaltyMeshedScene};
        let frame = SoccerPenaltyApp::build_stage1();
        let mut app = soccer_meshed_shell();
        let mut scene = PenaltyMeshedScene::install(&mut app);
        scene.author(&mut app, &frame); // first author bakes the skinned bodies once
        let outcome1 = app.tick(0);
        let meshes_after_1 = app.mesh_set().len() + app.skinned_mesh_set().len();
        scene.author(&mut app, &frame); // second author only re-poses; bakes nothing
        let _ = app.tick(1);
        let meshes_after_2 = app.mesh_set().len() + app.skinned_mesh_set().len();
        // The athletes are skinned draws (submitted per frame, geometry unchanged).
        assert!(!outcome1.skinned_draws().is_empty(), "athletes are skinned draws");
        // The core regression: after the one-time bind bake, authoring registers no
        // new meshes per frame.
        assert_eq!(meshes_after_1, meshes_after_2, "author must not bake new meshes per frame");
    }
}
