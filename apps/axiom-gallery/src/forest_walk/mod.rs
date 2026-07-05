//! # `forest_walk` — a first-person walk through the `prologue_postcard_001` forest.
//!
//! The visual target is a *static* diorama rendered from a fixed camera to a PNG.
//! This demo makes it **playable**: the same forest geometry (`build::build` over the
//! champion manifest) is uploaded once, and each frame a first-person camera — driven
//! by WASD + mouse-look, seated on the terrain surface (ground-follow) — re-projects
//! every instance and presents it live through the engine's `axiom-windowing`
//! WebGPU → WebGL2 → Canvas 2D cascade (`run_web_multi`).
//!
//! It matches the hero render's full instance counts: the trunks, foliage cards, and
//! ground clutter are the exact same batches the offscreen GPU path draws — only the
//! camera moves. Deterministic geometry; the god-ray volumetric post-pass is the one
//! thing the live arm does not yet apply (a documented shader follow-up).
//!
//! wasm32 only — it is the browser presentation arm; native builds compile it away.
#![cfg(target_arch = "wasm32")]

use std::cell::RefCell;
use std::rc::Rc;

use axiom_fp_controller::{FpController, Lens, LookDelta, MoveIntent, Pose, WalkTuning};
use axiom_kernel::{Meters, Radians, Ratio};
use axiom_math::Mat4;
use axiom_windowing::WindowingApi;
use wasm_bindgen::prelude::*;
use wasm_bindgen::JsCast;

use crate::growth::visual_target::build::{self, RenderData};
use crate::growth::visual_target::scene::Manifest;

/// The presentation canvas element id (must match `web/forest-walk/index.html`).
const CANVAS_ID: &str = "axiom-forest-walk-canvas";
const SURFACE_W: u32 = 1280;
const SURFACE_H: u32 = 800;

/// The champion scene, baked into the wasm bundle (the exact hero manifest).
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

/// The shared first-person walk tuning (rates, limits, look sensitivity) — the
/// engine's `axiom-fp-controller` owns these once for every first-person demo.
const TUNING: WalkTuning = WalkTuning::walk();
/// Mouse-look sensitivity (radians per pixel), sourced from the shared tuning so
/// the browser handler and the controller agree on one value.
const LOOK_SENS: f32 = TUNING.look_sensitivity().get();

/// Boot the first-person forest walk on the demo canvas.
#[wasm_bindgen]
pub fn forest_walk_start() {
    console_error_panic_hook::set_once();
    let manifest = match Manifest::parse(MANIFEST) {
        Ok(m) => m,
        Err(e) => {
            log(&format!("[forest_walk] manifest parse failed: {e}"));
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
    let spawn = Pose::new(
        Meters::finite_or_zero(manifest.camera.eye[0]),
        Meters::finite_or_zero(manifest.camera.eye[2]),
        Radians::finite_or_zero(0.0),
        Radians::finite_or_zero(-0.05),
    );
    let pose = Rc::new(RefCell::new(spawn));
    let keys = Rc::new(RefCell::new(MoveIntent::default()));
    // Accumulated mouse-look (yaw, pitch) radians, drained each frame.
    let look = Rc::new(RefCell::new((0.0f32, 0.0f32)));
    install_key_listener(&keys, "keydown", true);
    install_key_listener(&keys, "keyup", false);
    install_pointer_lock();
    install_mouse_look(&look);

    let mut windowing = WindowingApi::new();
    if windowing.configure_surface(SURFACE_W, SURFACE_H).is_err() {
        log("[forest_walk] invalid surface");
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
            let (ly, lp) = {
                let mut b = look.borrow_mut();
                let v = *b;
                *b = (0.0, 0.0);
                v
            };
            let look = LookDelta::new(Radians::finite_or_zero(ly), Radians::finite_or_zero(lp));
            let mut p = pose.borrow_mut();
            *p = FpController::step(*p, k, look, TUNING);
            let ground = terrain.height_at(p.x().get(), p.z().get());
            let lens = Lens::new(
                Radians::finite_or_zero(fov.to_radians()),
                Ratio::finite_or_zero(SURFACE_W as f32 / SURFACE_H as f32),
                Meters::finite_or_zero(near),
                Meters::finite_or_zero(far),
            );
            let vp = FpController::view_projection(*p, Meters::finite_or_zero(ground), TUNING, lens);
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
/// live backend's per-instance `mvp(16) · world(16) · tint(4)` layout (the same
/// 36-float instance the offscreen path emits): the shader clips with `mvp` and
/// lights with `world`, so both matrices must ride along — only `mvp` changes as the
/// camera moves.
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
fn install_key_listener(keys: &Rc<RefCell<MoveIntent>>, event: &str, pressed: bool) {
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
    let _ = document()
        .add_event_listener_with_callback(event, cb.as_ref().unchecked_ref());
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
fn install_mouse_look(look: &Rc<RefCell<(f32, f32)>>) {
    let look = look.clone();
    let cb = Closure::<dyn FnMut(web_sys::MouseEvent)>::new(move |e: web_sys::MouseEvent| {
        if pointer_is_locked() {
            let mut l = look.borrow_mut();
            l.0 += e.movement_x() as f32 * LOOK_SENS;
            l.1 -= e.movement_y() as f32 * LOOK_SENS;
        }
    });
    let _ = document().add_event_listener_with_callback("mousemove", cb.as_ref().unchecked_ref());
    cb.forget();
}
