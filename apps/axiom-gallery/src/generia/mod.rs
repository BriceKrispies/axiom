//! # `generia` — a first-person walk through a procedural Axiom forest.
//!
//! This is the port target for the old WAT-engine "fall-forest" game: the game's
//! systems (chunked world-gen, layered terrain, rule-based props, discoveries,
//! world modes, a horror layer, a narrative console) are being rebuilt on the
//! Axiom engine, wearing Axiom's GPU forest look instead of a 320×200 software
//! rasterizer.
//!
//! **Phase 1 (this module) is the foundation:** the walkable GPU forest itself.
//! The same generators the offscreen hero render uses (`build::build` over a
//! forest manifest) are uploaded once, and each frame a first-person camera —
//! WASD + mouse-look, seated on the terrain (ground-follow) — re-projects every
//! instance and presents it live through `axiom-windowing`'s WebGPU → WebGL2 →
//! Canvas 2D cascade (`run_web_multi`). Subsequent phases replace the fixed
//! manifest with streamed procedural chunks and layer the game systems on top.
//!
//! wasm32 only — it is the browser presentation arm; native builds compile it away.
#![cfg(target_arch = "wasm32")]

use std::cell::RefCell;
use std::rc::Rc;

use axiom_math::{Mat4, Vec3};
use axiom_windowing::WindowingApi;
use wasm_bindgen::prelude::*;
use wasm_bindgen::JsCast;

use crate::growth::visual_target::build::{self, RenderData};
use crate::growth::visual_target::scene::Manifest;

/// The presentation canvas element id (must match `web/generia/index.html`).
const CANVAS_ID: &str = "axiom-generia-canvas";
const SURFACE_W: u32 = 1280;
const SURFACE_H: u32 = 800;

/// Eye height above the forest floor, and movement/turn/look rates (per frame / per px).
const EYE_HEIGHT_M: f32 = 1.7;
const MOVE_SPEED_M: f32 = 0.22;
const TURN_SPEED: f32 = 0.028;
const LOOK_SENS: f32 = 0.0022;
const PITCH_LIMIT: f32 = 1.45;

/// The forest scene, baked into the wasm bundle. Phase 1 reuses the champion
/// forest manifest; later phases generate chunks procedurally instead.
const MANIFEST: &str = include_str!("../../visual_targets/prologue_postcard_001/manifest.toml");

/// One instance's fixed world transform + tint (camera-independent).
struct Inst {
    world: Mat4,
    tint: [f32; 4],
}

/// One `(mesh, material)` batch of instances + their contact-shadow caster flags.
struct MeshBatch {
    mesh_id: u64,
    material_id: u64,
    insts: Vec<Inst>,
    casts: bool,
}

/// The player's first-person pose.
#[derive(Clone, Copy, Default)]
struct Pose {
    x: f32,
    z: f32,
    yaw: f32,
    pitch: f32,
}

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
#[derive(Clone, Copy, Default)]
struct Look {
    yaw: f32,
    pitch: f32,
}

/// Boot the first-person generia forest walk on the demo canvas.
#[wasm_bindgen]
pub fn generia_start() {
    console_error_panic_hook::set_once();
    let manifest = match Manifest::parse(MANIFEST) {
        Ok(m) => m,
        Err(e) => {
            log(&format!("[generia] manifest parse failed: {e}"));
            return;
        }
    };
    let rd = build::build(&manifest);
    let batches = split_batches(&rd);
    let (near, far, fov) = (manifest.camera.near_m, manifest.camera.far_m, manifest.camera.fov_deg);
    let clear = rd.clear;
    let lights = rd.lights.clone();
    let light_vp = rd.light_view_proj;

    // Spawn where the hero camera stands, but seated on the ground and free to walk.
    let spawn = Pose {
        x: manifest.camera.eye[0],
        z: manifest.camera.eye[2],
        yaw: 0.0,
        pitch: -0.05,
    };
    let pose = Rc::new(RefCell::new(spawn));
    let keys = Rc::new(RefCell::new(Keys::default()));
    let look = Rc::new(RefCell::new(Look::default()));
    install_key_listener(&keys, "keydown", true);
    install_key_listener(&keys, "keyup", false);
    install_pointer_lock();
    install_mouse_look(&look);

    let mut windowing = WindowingApi::new();
    if windowing.configure_surface(SURFACE_W, SURFACE_H).is_err() {
        log("[generia] invalid surface");
        return;
    }
    let terrain = manifest.terrain.clone();
    // The live backend packs every batch into ONE instance buffer at cumulative
    // offsets, so the capacity must be the TOTAL instance count across all batches.
    let max_instances = batches.iter().map(|b| b.insts.len() as u32).sum::<u32>().max(1);
    let casters = flat_casters(&batches);

    let _ = windowing.run_web_multi(
        CANVAS_ID,
        rd.meshes.clone(),
        rd.materials.clone(),
        max_instances,
        move |_tick| {
            let k = *keys.borrow();
            let l = {
                let mut b = look.borrow_mut();
                let v = *b;
                *b = Look::default();
                v
            };
            let mut p = pose.borrow_mut();
            step(&mut p, &k, l);
            let ground = terrain.height_at(p.x, p.z);
            let vp = camera_view_proj(&p, ground, fov, near, far);
            let out = project_batches(&batches, &vp);
            (clear, lights.clone(), light_vp, out, vp.as_cols_array(), casters.clone(), None)
        },
    );
}

/// Split the render data's `[mvp, world, tint]` batches into camera-independent
/// world-transform + tint instances, grouped by `(mesh, material)`.
fn split_batches(rd: &RenderData) -> Vec<MeshBatch> {
    rd.batches
        .iter()
        .map(|(mesh_id, material_id, data, count)| {
            let insts = (0..*count as usize)
                .map(|i| {
                    let base = i * 36;
                    Inst {
                        world: Mat4::from_cols_array(slice16(&data[base + 16..base + 32])),
                        tint: [data[base + 32], data[base + 33], data[base + 34], data[base + 35]],
                    }
                })
                .collect();
            // Trees (trunk mesh 2, canopy/foliage mesh 3 & 5) cast contact shadows.
            let casts = *mesh_id == 2 || *mesh_id == 3 || *mesh_id == 5;
            MeshBatch { mesh_id: *mesh_id, material_id: *material_id, insts, casts }
        })
        .collect()
}

/// The per-instance caster flags in batch-expansion order (for the planar-shadow pass).
fn flat_casters(batches: &[MeshBatch]) -> Vec<bool> {
    batches.iter().flat_map(|b| std::iter::repeat(b.casts).take(b.insts.len())).collect()
}

/// Re-project every instance through the current camera view-projection into the
/// live backend's per-instance `mvp(16) · world(16) · tint(4)` layout.
fn project_batches(batches: &[MeshBatch], vp: &Mat4) -> Vec<(u64, u64, Vec<f32>, u32)> {
    batches
        .iter()
        .map(|b| {
            let mut data = Vec::with_capacity(b.insts.len() * 36);
            b.insts.iter().for_each(|i| {
                data.extend_from_slice(&vp.multiply(i.world).as_cols_array());
                data.extend_from_slice(&i.world.as_cols_array());
                data.extend_from_slice(&i.tint);
            });
            (b.mesh_id, b.material_id, data, b.insts.len() as u32)
        })
        .collect()
}

/// Integrate one frame of first-person movement + look.
fn step(p: &mut Pose, k: &Keys, l: Look) {
    let key_turn = (k.turn_left as i32 - k.turn_right as i32) as f32 * TURN_SPEED;
    p.yaw += key_turn + l.yaw;
    p.pitch = (p.pitch + l.pitch).clamp(-PITCH_LIMIT, PITCH_LIMIT);
    let (fx, fz) = (p.yaw.sin(), -p.yaw.cos()); // yaw 0 ⇒ facing −Z
    let fwd = (k.forward as i32 - k.backward as i32) as f32 * MOVE_SPEED_M;
    let strafe = (k.strafe_right as i32 - k.strafe_left as i32) as f32 * MOVE_SPEED_M;
    p.x += fx * fwd + fz * -strafe;
    p.z += fz * fwd - fx * -strafe;
}

/// The camera view-projection for the current pose, eye seated on the terrain.
fn camera_view_proj(p: &Pose, ground: f32, fov_deg: f32, near: f32, far: f32) -> Mat4 {
    let eye = Vec3::new(p.x, ground + EYE_HEIGHT_M, p.z);
    let (cp, sp) = (p.pitch.cos(), p.pitch.sin());
    let fwd = Vec3::new(p.yaw.sin() * cp, sp, -p.yaw.cos() * cp);
    let target = Vec3::new(eye.x + fwd.x, eye.y + fwd.y, eye.z + fwd.z);
    let aspect = SURFACE_W as f32 / SURFACE_H as f32;
    let proj = Mat4::perspective(fov_deg.to_radians(), aspect, near, far).unwrap_or(Mat4::IDENTITY);
    let view = Mat4::look_at(eye, target, Vec3::UNIT_Y).unwrap_or(Mat4::IDENTITY);
    proj.multiply(view)
}

fn slice16(s: &[f32]) -> [f32; 16] {
    let mut m = [0.0f32; 16];
    m.copy_from_slice(s);
    m
}

// --- web glue -------------------------------------------------------------------

fn log(msg: &str) {
    web_sys::console::log_1(&JsValue::from_str(msg));
}

fn document() -> web_sys::Document {
    web_sys::window().and_then(|w| w.document()).expect("a document")
}

fn pointer_is_locked() -> bool {
    document().pointer_lock_element().is_some()
}

/// Install a keydown/keyup listener that sets the held-keys state.
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
fn install_pointer_lock() {
    if let Some(canvas) = document().get_element_by_id(CANVAS_ID) {
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
            l.yaw += e.movement_x() as f32 * LOOK_SENS;
            l.pitch -= e.movement_y() as f32 * LOOK_SENS;
        }
    });
    let _ = document().add_event_listener_with_callback("mousemove", cb.as_ref().unchecked_ref());
    cb.forget();
}
