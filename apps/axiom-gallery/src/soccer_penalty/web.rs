//! The soccer-penalty gallery arm: the native **engine-scene bridge** that
//! renders the game's render plan through the real engine backend, plus the
//! `wasm32` browser entry (`soccer_penalty_start`) that drives the windowing run
//! loop, steps the session from keyboard/pad input, and re-authors the scene
//! every frame — exactly the `physics_crucible` live pattern.
//!
//! The render plan's flat, pre-shaded colours (Pass 3) are carried as
//! **emissive** on a black-base material, so the engine renders them unlit —
//! byte-faithful to the diorama's own shading without needing the engine light
//! model. Boxes/quads/lines become the unit cube (extent 1 → scale = size), the
//! ball becomes the unit sphere (radius 0.5 → scale = size·2), and every draw is
//! nudged a hair toward the camera in the plan's back-to-front order so the many
//! near-coplanar ground/net quads win the depth test cleanly.

use axiom::prelude::*;

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

use crate::soccer_penalty::low_poly_assets::PrimitiveShape;
use crate::soccer_penalty::penalty_render_plan::PenaltyRenderContent;
use crate::soccer_penalty::soccer_penalty_app::Stage1Diorama;
// Only the wasm entry + native tests build a session; the scene author takes a frame.
#[cfg(any(test, target_arch = "wasm32"))]
use crate::soccer_penalty::SoccerPenaltyApp;

const CANVAS_ID: &str = "axiom-soccer-penalty-canvas";
const WIDTH: u32 = 960;
const HEIGHT: u32 = 600;
/// The live instance cap: the diorama draws ~100 objects, well under this.
#[cfg(target_arch = "wasm32")]
const CAPACITY: u32 = 1024;

/// A finite `Ratio` from a colour channel (clamped, so always valid).
fn ch(value: f32) -> Ratio {
    Ratio::new(value.clamp(0.0, 1.0)).expect("clamped colour channel is finite")
}

/// Keep flat quads genuinely thin so a ground layer's slab does not overlap the
/// layer a few millimetres above it (the unit cube has extent 1, so scale=size).
fn nonzero(s: Vec3) -> Vec3 {
    let c = |v: f32| if v.abs() < 1.0e-3 { 0.01 } else { v };
    Vec3::new(c(s.x), c(s.y), c(s.z))
}

/// Author the current diorama frame into the engine scene: one cube/sphere
/// `Renderable` per world render item (emissive flat colour), a fixed camera,
/// and a token light.
pub fn author_soccer(
    world: &mut SceneCommands,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<Material>,
    frame: &Stage1Diorama,
) {
    let cam = frame.render_plan.camera;
    let cube = meshes.add(Mesh::cube());
    let sphere = meshes.add(Mesh::sphere());

    let mut index = 0u64;
    frame.render_plan.items.iter().for_each(|item| {
        if let PenaltyRenderContent::World { shape, position, size, shaded_color, .. } = item.content {
            let (mesh, scale) = match shape {
                PrimitiveShape::FacetedBall => (sphere, size.mul_scalar(2.0)),
                _ => (cube, nonzero(size)),
            };
            // Painter's-order depth bias toward the camera (see module docs).
            let to_eye = cam.eye.subtract(position);
            let dir = to_eye.mul_scalar(1.0 / to_eye.length().max(1.0e-6));
            let biased = position.add(dir.mul_scalar(index as f32 * 0.0015));
            let material = materials.add(
                Material::lit(Color::BLACK).with_emissive(Color::linear_rgb(
                    ch(shaded_color.r),
                    ch(shaded_color.g),
                    ch(shaded_color.b),
                )),
            );
            world.spawn((
                Transform::combine(Transform::from_translation(biased), Transform::from_scale(scale)),
                Renderable { mesh, material },
            ));
            index += 1;
        }
    });

    world.spawn((
        Transform::from_translation(cam.eye)
            .looking_at(cam.target, cam.up)
            .unwrap_or_else(|_| Transform::from_translation(cam.eye)),
        Camera::perspective(PerspectiveProjection {
            fov_y: Angle::degrees(cam.fov_y_degrees),
            near: Meters::new(cam.near).expect("camera near plane is finite"),
            far: Meters::new(cam.far).expect("camera far plane is finite"),
        }),
    ));
    // Emissive is self-lit, but keep one directional light so any lit fallback
    // still resolves a frame.
    world.spawn((
        Transform::IDENTITY,
        DirectionalLight {
            direction: Vec3::new(0.3, -1.0, 0.4),
            color: Color::WHITE,
            intensity: ch(1.0),
        },
    ));
}

/// Build the initial live [`RunningApp`] for the given frame — the browser loop
/// drives and re-authors it each tick.
pub fn soccer_live_app(frame: Stage1Diorama) -> RunningApp {
    App::new()
        .window(
            Window::new(WIDTH, HEIGHT)
                .with_surface_id(CANVAS_ID)
                .with_clear_color(Color::linear_rgb(ch(0.07), ch(0.10), ch(0.18))),
        )
        .add_plugins(DefaultPlugins)
        .setup(move |world, meshes, materials| author_soccer(world, meshes, materials, &frame))
        .build()
}

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

    let session = SoccerPenaltyApp::new_session();
    let running = soccer_live_app(SoccerPenaltyApp::build_session_frame(&session));
    let meshes = running.mesh_set();
    let materials = running.material_textures();

    let state = Rc::new(RefCell::new((session, running)));
    let frame_input = input.clone();
    let frame = move |tick: u64| {
        let mut guard = state.borrow_mut();
        let (session, running) = &mut *guard;
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
        running.reauthor(move |world, meshes, materials| {
            author_soccer(world, meshes, materials, &diorama)
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
        let frame = SoccerPenaltyApp::build_stage1();
        let world_items = frame
            .render_plan
            .items
            .iter()
            .filter(|it| matches!(it.content, PenaltyRenderContent::World { .. }))
            .count();
        let mut app = soccer_live_app(frame);
        // Every world render item becomes one renderable; the camera + light are
        // not renderables.
        assert_eq!(app.renderable_count(), world_items);
        let outcome = app.tick(0);
        assert_eq!(outcome.draws().len(), world_items);
    }
}
