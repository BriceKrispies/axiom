//! # `generia` — a first-person walk through an **endless** procedural Axiom forest.
//!
//! The port target for the WAT-engine fall-forest game, on Axiom's GPU forest.
//! Phase 2: the world **streams**. A [`axiom_world::WorldApi`] residency ring
//! loads/unloads/culls chunks around the camera; each loaded chunk's trees are
//! placed by [`axiom_scatter::ScatterApi`] and turned into the same rich
//! trunk/foliage/branch instances the hero render uses (`build::*_instances`);
//! the ground is a terrain mesh regenerated around the moving camera and streamed
//! into the backend via `run_web_multi_streaming`. Walk forever — chunks appear
//! ahead and unload behind, and only the visible ones are drawn.
//!
//! Later phases layer on the fall-forest game systems (layered terrain + rail
//! path, rule-based props, discoveries, world modes, the horror layer, a console).
//!
//! wasm32 only — the browser presentation arm; native builds compile it away.
#![cfg(target_arch = "wasm32")]

use std::cell::RefCell;
use std::collections::BTreeMap;
use std::rc::Rc;

use axiom_agent_harness::AgentHarnessApi;
use axiom_fp_controller::{FpController, Lens, LookDelta, MoveIntent, Pose, WalkTuning};
use axiom_kernel::{Meters, Radians, Ratio, StableHash};
use axiom_math::{Aabb, Mat4, Vec3};
use axiom_scatter::{CellCoord, ScatterApi, ScatterRule, ScatterSite};
use axiom_streaming::ChunkCoord;
use axiom_windowing::WindowingApi;
use axiom_world::{WorldApi, WorldConfig};
use wasm_bindgen::prelude::*;
use wasm_bindgen::JsCast;

use crate::growth::visual_target::build::{
    self, branch_instances, foliage_instances, terrain_window_mesh, trunk_instances,
};
use crate::growth::visual_target::scene::{Foliage, Manifest, Tree};

const CANVAS_ID: &str = "axiom-generia-canvas";
const SURFACE_W: u32 = 1280;
const SURFACE_H: u32 = 800;

/// Mesh + material ids (must match `build.rs`).
const TERRAIN_MESH: u64 = 1;
const TRUNK_MESH: u64 = 2;
const FOLIAGE_MESH: u64 = 5;
const BRANCH_MESH: u64 = 7;
const WHITE_MAT: u64 = 1;
const LEAF_ALPHA_MAT: u64 = 2;
const BARK_MAT: u64 = 3;
const GROUND_MAT: u64 = 4;

/// Streaming shape: chunk size, load ring, and the terrain-window spacing.
const CHUNK_M: f32 = 24.0;
const LOAD_RADIUS: i32 = 3;
const MARGIN: i32 = 1;
const TREE_SEED: u64 = 0x_67_65_6e_65_72_69_61_00; // "generia\0"
const TERRAIN_SPACING_M: f32 = 1.0; // coarser than the hero patch, for a big window
/// Upper bound on instances drawn in a frame (backend instance-buffer capacity).
const MAX_INSTANCES: u32 = 180_000;

/// Autonomous benchmark walk (`?agent=1`): the camera is driven by the first-person
/// agent harness around a deterministic waypoint ring, so a render benchmark gets a
/// reproducible walk through the streaming world.
const AGENT_ID: u64 = 0x_67_65_6e_65_72_69_61; // "generia"
const WALK_RADIUS_M: f32 = 45.0;
const ARRIVE_RADIUS_M: f32 = 3.0;
const AGENT_WAYPOINTS: u32 = 8;

/// The shared first-person walk tuning (rates, limits, look sensitivity), owned
/// once by the engine's `axiom-fp-controller` for every first-person demo.
const TUNING: WalkTuning = WalkTuning::walk();
/// Eye height above ground, sourced from the shared tuning (the agent's pose math
/// needs it explicitly).
const EYE_HEIGHT_M: f32 = TUNING.eye_height().get();
/// Mouse-look sensitivity (radians per pixel), sourced from the shared tuning.
const LOOK_SENS: f32 = TUNING.look_sensitivity().get();

const IDENTITY16: [f32; 16] =
    [1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0];

/// One instance's fixed world transform + tint (camera-independent).
#[derive(Clone, Copy)]
struct Inst {
    world: Mat4,
    tint: [f32; 4],
}

/// A loaded chunk's cached vegetation, grouped by mesh.
struct ChunkVeg {
    trunk: Vec<Inst>,
    foliage: Vec<Inst>,
    branch: Vec<Inst>,
}

/// Boot the endless streamed generia forest on the demo canvas.
#[wasm_bindgen]
pub fn generia_start() {
    console_error_panic_hook::set_once();
    let manifest = match Manifest::parse(include_str!(
        "../../visual_targets/prologue_postcard_001/manifest.toml"
    )) {
        Ok(m) => m,
        Err(e) => {
            log(&format!("[generia] manifest parse failed: {e}"));
            return;
        }
    };
    // Build once for the unit meshes + materials; the baked batches are ignored —
    // generia generates its own per-chunk instances.
    let rd = build::build(&manifest);
    let (near, far, fov) = (manifest.camera.near_m, manifest.camera.far_m, manifest.camera.fov_deg);
    let clear = rd.clear;
    let lights = rd.lights.clone();
    let light_vp = rd.light_view_proj;
    let foliage = match manifest.foliage.clone() {
        Some(f) => f,
        None => {
            log("[generia] manifest has no [foliage]");
            return;
        }
    };
    // Canvas2D projects every leaf card on the CPU (single-threaded) — that projection
    // is ~96% of its frame cost — so the full canopy density (hundreds of cards/tree)
    // is unplayable there. On the software backend, drop to a low-detail canopy: far
    // fewer, larger cards. The GPU keeps the full density.
    let low_detail = low_detail_backend();
    let foliage = if low_detail {
        low_detail_foliage(&foliage)
    } else {
        foliage
    };
    let terrain = manifest.terrain.clone();
    // A coarser terrain clone for the streamed window (fewer verts over a big area).
    // Coarser still on the software backend: it halves the terrain triangle count the
    // CPU projects AND shrinks the per-chunk-crossing regen spike (the `worst` frame).
    let mut coarse_terrain = terrain.clone();
    coarse_terrain.spacing_m = if low_detail {
        TERRAIN_SPACING_M * 2.0
    } else {
        TERRAIN_SPACING_M
    };
    let manifest = Rc::new(manifest);

    let spawn = Pose::new(
        Meters::finite_or_zero(0.0),
        Meters::finite_or_zero(0.0),
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
        log("[generia] invalid surface");
        return;
    }

    // On the software backend, a shorter residency ring cuts *every* draw (trunks +
    // branches + terrain window), not just foliage — and the fog hides the nearer
    // view distance. The GPU keeps the full radius.
    let load_radius = if low_detail { 2 } else { LOAD_RADIUS };
    let world = Rc::new(RefCell::new(WorldApi::new(WorldConfig {
        chunk_size: Meters::finite_or_zero(CHUNK_M),
        load_radius,
        margin: MARGIN,
        lod_bands: vec![Meters::finite_or_zero(80.0), Meters::finite_or_zero(160.0)],
    })));
    let cache: Rc<RefCell<BTreeMap<(i32, i32), ChunkVeg>>> = Rc::new(RefCell::new(BTreeMap::new()));
    let last_focus: Rc<RefCell<Option<(i32, i32)>>> = Rc::new(RefCell::new(None));

    // Autonomous benchmark walk (`?agent=1`): a deterministic waypoint ring the
    // agent circles, exercising streaming + varied views for the render benchmark.
    let agent_mode = agent_preference();
    let waypoints = agent_waypoints();
    let wp_idx = Rc::new(RefCell::new(0usize));

    let _ = windowing.run_web_multi_streaming(
        CANVAS_ID,
        rd.meshes.clone(),
        rd.materials.clone(),
        TERRAIN_MESH,
        MAX_INSTANCES,
        move |tick| {
            let mut k = *keys.borrow();
            let (ly, lp) = {
                let mut b = look.borrow_mut();
                let v = *b;
                *b = (0.0, 0.0);
                v
            };
            let look = LookDelta::new(Radians::finite_or_zero(ly), Radians::finite_or_zero(lp));
            let mut p = pose.borrow_mut();
            if agent_mode {
                let ground = terrain.height_at(p.x().get(), p.z().get());
                let control = agent_control(&p, ground, tick, &waypoints, &mut wp_idx.borrow_mut());
                apply_control(&mut k, control);
            }
            *p = FpController::step(*p, k, look, TUNING);
            let ground = terrain.height_at(p.x().get(), p.z().get());
            let eye = FpController::eye_position(*p, Meters::finite_or_zero(ground), TUNING);
            let lens = Lens::new(
                Radians::finite_or_zero(fov.to_radians()),
                Ratio::finite_or_zero(SURFACE_W as f32 / SURFACE_H as f32),
                Meters::finite_or_zero(near),
                Meters::finite_or_zero(far),
            );
            let vp = FpController::view_projection(*p, Meters::finite_or_zero(ground), TUNING, lens);

            // Plan the frame: load / unload / visible chunks.
            let plan = world.borrow_mut().frame_plan(eye, vp, chunk_aabb);
            {
                let mut c = cache.borrow_mut();
                for coord in &plan.load {
                    c.insert((coord.x, coord.z), gen_chunk_veg(&manifest, &foliage, *coord, eye));
                }
                for coord in &plan.unload {
                    c.remove(&(coord.x, coord.z));
                }
            }

            // Regenerate the terrain window when the camera crosses into a new chunk.
            let focus = (
                (p.x().get() / CHUNK_M).floor() as i32,
                (p.z().get() / CHUNK_M).floor() as i32,
            );
            let new_geometry = {
                let mut lf = last_focus.borrow_mut();
                if *lf != Some(focus) {
                    *lf = Some(focus);
                    let cx = (focus.0 as f32 + 0.5) * CHUNK_M;
                    let cz = (focus.1 as f32 + 0.5) * CHUNK_M;
                    let radius = (load_radius as f32 + 1.0) * CHUNK_M;
                    Some(terrain_window_mesh(
                        &coarse_terrain,
                        &manifest.fog,
                        eye,
                        &style_of(&manifest),
                        (cx, cz),
                        radius,
                    ))
                } else {
                    None
                }
            };

            // Gather visible chunks' instances into the three vegetation batches.
            // Distance LOD (WorldApi hands a level per visible chunk): the leaf-card
            // foliage is the triangle hog, so only the nearest band (lod 0) draws it;
            // farther chunks keep just trunks + branches (readable through the fog).
            let c = cache.borrow();
            let (mut trunk, mut foliage_d, mut branch) = (Vec::new(), Vec::new(), Vec::new());
            let (mut tn, mut fn_, mut bn) = (0u32, 0u32, 0u32);
            for vc in &plan.visible {
                if let Some(veg) = c.get(&(vc.coord.x, vc.coord.z)) {
                    tn += project_into(&mut trunk, &veg.trunk, &vp);
                    bn += project_into(&mut branch, &veg.branch, &vp);
                    if vc.lod == 0 {
                        fn_ += project_into(&mut foliage_d, &veg.foliage, &vp);
                    }
                }
            }

            let terrain_inst = terrain_instance(&vp);
            let batches = vec![
                (TERRAIN_MESH, GROUND_MAT, terrain_inst, 1),
                (TRUNK_MESH, BARK_MAT, trunk, tn),
                (FOLIAGE_MESH, LEAF_ALPHA_MAT, foliage_d, fn_),
                (BRANCH_MESH, WHITE_MAT, branch, bn),
            ];
            (clear, lights.clone(), light_vp, batches, vp.as_cols_array(), Vec::new(), None, new_geometry)
        },
    );
}

/// The manifest's style (or the neutral default).
fn style_of(m: &Manifest) -> crate::growth::visual_target::scene::Style {
    m.style.clone().unwrap_or_else(crate::growth::visual_target::scene::Style::neutral)
}

/// Generate one chunk's cached vegetation: scatter its trees, then run the hero
/// trunk/foliage/branch generators over that chunk's trees (with an identity
/// view-projection, so the emitted `mvp` slot equals the world transform we keep).
fn gen_chunk_veg(manifest: &Manifest, foliage: &Foliage, cell: ChunkCoord, eye: Vec3) -> ChunkVeg {
    let rule = ScatterRule {
        sites_per_side: 3,
        jitter: Ratio::new(0.8).unwrap_or_else(|_| Ratio::new(0.0).unwrap()),
        fill: Ratio::new(0.7).unwrap_or_else(|_| Ratio::new(0.0).unwrap()),
    };
    let sites = ScatterApi::chunk_sites(
        TREE_SEED,
        CellCoord::new(cell.x, cell.z),
        Meters::finite_or_zero(CHUNK_M),
        &rule,
    );
    let trees: Vec<Tree> = sites.iter().map(|s| site_to_tree(manifest, s)).collect();
    let lean = manifest.scatter.as_ref().map(|s| s.lean_deg).unwrap_or(0.0);
    ChunkVeg {
        trunk: extract(&trunk_instances(manifest, &trees, lean, &IDENTITY16, eye)),
        foliage: extract(&foliage_instances(manifest, &trees, foliage, lean, &IDENTITY16, eye)),
        branch: extract(&branch_instances(manifest, &trees, foliage, lean, &IDENTITY16, eye)),
    }
}

/// A scattered site → a `Tree`, its size/rotation/colour drawn deterministically
/// from the site's seed across the manifest scatter's ranges.
fn site_to_tree(manifest: &Manifest, s: &ScatterSite) -> Tree {
    let (th, tr, cr, pal): ([f32; 2], [f32; 2], [f32; 2], &[[f32; 3]]) = match &manifest.scatter {
        Some(sc) => (sc.trunk_height_m, sc.trunk_radius_m, sc.canopy_radius_m, sc.canopy_palette.as_slice()),
        None => ([6.0, 14.0], [0.2, 0.5], [2.0, 4.0], &[[0.8, 0.5, 0.2]]),
    };
    let h = StableHash::of_words(&[s.seed]).raw();
    let u = |shift: u32| ((h >> shift) & 0xFFFF) as f32 / 65_536.0;
    let lerp = |r: [f32; 2], t: f32| r[0] + (r[1] - r[0]) * t;
    let color = pal.get(((h >> 8) as usize) % pal.len().max(1)).copied().unwrap_or([0.8, 0.5, 0.2]);
    Tree {
        x: s.x.get(),
        z: s.z.get(),
        yaw_deg: u(0) * 360.0,
        trunk_height_m: lerp(th, u(16)),
        trunk_radius_m: lerp(tr, u(32)),
        canopy_radius_m: lerp(cr, u(48)),
        canopy_color: color,
    }
}

/// A chunk's world-space bounding box for frustum culling / LOD.
fn chunk_aabb(c: ChunkCoord) -> Aabb {
    let x = c.x as f32 * CHUNK_M;
    let z = c.z as f32 * CHUNK_M;
    Aabb::new(Vec3::new(x, -12.0, z), Vec3::new(x + CHUNK_M, 48.0, z + CHUNK_M))
        .unwrap_or_else(|_| Aabb::from_center_extents(Vec3::ZERO, Vec3::ONE).unwrap())
}

/// Split a `[mvp, world, tint]` instance stream into camera-independent
/// `Inst { world, tint }` (the generators are called with an identity vp, so the
/// `world` slot at floats 16..32 is the true world transform).
fn extract(data: &[f32]) -> Vec<Inst> {
    (0..data.len() / 36)
        .map(|i| {
            let b = i * 36;
            Inst {
                world: Mat4::from_cols_array(slice16(&data[b + 16..b + 32])),
                tint: [data[b + 32], data[b + 33], data[b + 34], data[b + 35]],
            }
        })
        .collect()
}

/// Re-project a chunk's cached instances into the live `[mvp, world, tint]` layout,
/// appending onto `out`; returns how many were appended.
fn project_into(out: &mut Vec<f32>, insts: &[Inst], vp: &Mat4) -> u32 {
    for i in insts {
        out.extend_from_slice(&vp.multiply(i.world).as_cols_array());
        out.extend_from_slice(&i.world.as_cols_array());
        out.extend_from_slice(&i.tint);
    }
    insts.len() as u32
}

/// The single terrain instance: world coords already, so world = identity and
/// mvp = the camera view-projection.
fn terrain_instance(vp: &Mat4) -> Vec<f32> {
    let mut d = Vec::with_capacity(36);
    d.extend_from_slice(&vp.as_cols_array());
    d.extend_from_slice(&IDENTITY16);
    d.extend_from_slice(&[1.0, 1.0, 1.0, 1.0]);
    d
}

// --- autonomous benchmark walk (agent) -----------------------------------------

/// Whether `?agent=1` is present in the URL — enables the autonomous walk.
fn agent_preference() -> bool {
    web_sys::window()
        .and_then(|w| w.location().search().ok())
        .map(|s| s.contains("agent=1"))
        .unwrap_or(false)
}

/// Whether the Canvas2D software backend was explicitly selected
/// (`?backend=canvas2d`) — the low-power path that projects every card on the CPU, so
/// it gets the thinned canopy. The GPU cascade (no `?backend`, or webgpu/webgl2) keeps
/// the full-density foliage.
fn low_detail_backend() -> bool {
    web_sys::window()
        .and_then(|w| w.location().search().ok())
        .map(|s| s.contains("backend=canvas2d"))
        .unwrap_or(false)
}

/// A drastically thinned canopy for the Canvas2D software backend. The leaf-card count
/// (`branches * leaves_per_branch`, plus the loose `cards_per_tree`/`understory_cards`)
/// is the projection hog — ~96% of the Canvas2D frame — so cut it hard and enlarge each
/// remaining card so the canopy still reads as a coloured mass at 240×150.
fn low_detail_foliage(f: &Foliage) -> Foliage {
    let mut lo = f.clone();
    lo.branches = (f.branches / 2).max(2);
    lo.leaves_per_branch = (f.leaves_per_branch / 8).max(3);
    lo.cards_per_tree = (f.cards_per_tree / 4).max(2);
    lo.understory_cards = 0;
    lo.card_scale = f.card_scale * 1.8;
    lo
}

/// The deterministic waypoint ring the agent circles (a big loop through the
/// streaming world, so chunks load/unload + the view varies across the run).
fn agent_waypoints() -> Vec<(f32, f32)> {
    (0..AGENT_WAYPOINTS)
        .map(|i| {
            let a = i as f32 / AGENT_WAYPOINTS as f32 * std::f32::consts::TAU;
            (WALK_RADIUS_M * a.cos(), WALK_RADIUS_M * a.sin())
        })
        .collect()
}

/// One agent tick: seek the current waypoint (turn-toward + forward) through the
/// first-person control harness, advancing to the next waypoint on arrival. Returns
/// the control bitmask to fold into the keys.
fn agent_control(p: &Pose, ground: f32, tick: u64, waypoints: &[(f32, f32)], wp_idx: &mut usize) -> u32 {
    let m = |x: f32| AgentHarnessApi::micro(Meters::finite_or_zero(x));
    let (px, pz, pyaw) = (p.x().get(), p.z().get(), p.yaw().get());
    let pose_micro = (m(px), m(ground + EYE_HEIGHT_M), m(pz), m(pyaw));
    // The first-person forward convention (see `FpController::step`): (sin yaw, -cos yaw).
    let forward_micro = (m(pyaw.sin()), m(-pyaw.cos()));
    let (gx, gz) = waypoints[*wp_idx % waypoints.len()];
    let goal_micro = (m(gx), m(ground), m(gz));
    let (control, _reason, _brain, _emitted, arrived) =
        AgentHarnessApi::decide_goto(AGENT_ID, tick, pose_micro, forward_micro, goal_micro, m(ARRIVE_RADIUS_M));
    if arrived == 1 {
        *wp_idx = (*wp_idx + 1) % waypoints.len();
    }
    control
}

/// Fold an agent control bitmask into the keyboard keys (OR, so a human at the
/// keyboard can still nudge the autonomous walk).
fn apply_control(k: &mut MoveIntent, control: u32) {
    let has = |flag: u32| control & flag != 0;
    k.forward |= has(AgentHarnessApi::FORWARD);
    k.backward |= has(AgentHarnessApi::BACKWARD);
    k.turn_left |= has(AgentHarnessApi::TURN_LEFT);
    k.turn_right |= has(AgentHarnessApi::TURN_RIGHT);
    k.strafe_left |= has(AgentHarnessApi::STRAFE_LEFT);
    k.strafe_right |= has(AgentHarnessApi::STRAFE_RIGHT);
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
    let _ = document().add_event_listener_with_callback(event, cb.as_ref().unchecked_ref());
    cb.forget();
}

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
