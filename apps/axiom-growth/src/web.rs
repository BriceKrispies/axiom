//! The in-browser (WASM) Growth viewer — **`wasm32` only**.
//!
//! A small screen state machine that mirrors the original Growth flow:
//!
//!   (A) CONFIG  — an HTML form (seed / preset / detail) → **Generate**.
//!   (B) OVERWORLD — an equirectangular biome+elevation **map** of the generated
//!       planet, drawn into a 2D `<canvas>` by sampling `sample_surface` over
//!       lat/long. A world map is directly clickable to pick a spot.
//!   (C) SELECT  — the player clicks a LAND pixel; the pixel maps to lat/long →
//!       a unit direction on the sphere. Ocean clicks are rejected with a hint.
//!   (D) DESCEND — `GameWorldLocalMap::anchored_at(atlas, picked_dir)` anchors the
//!       local tangent frame at that spot, a terrain mesh is built around it, and
//!       the first-person **walkable** WebGPU view starts there.
//!
//! The deterministic worldgen core is untouched: this is the thin presentation
//! edge, generating a planet and sampling its terrain. Never compiled on native.
//!
//! ## Ground-follow (the player walks ON the terrain, not through it)
//!
//! The engine's `Controller` is a free-fly camera: it integrates horizontal
//! movement but keeps a constant Y, so it clips through hills. We instead make
//! the camera a character that **follows the surface**. The app owns the player
//! state `(x, z, yaw, pitch)` and the camera's current engine-space Y. Each
//! frame: (1) integrate horizontal movement by yaw and step; (2) sample the
//! ground height under the new `(x, z)` via `sample_height_m` (recentred by the
//! anchor height, then `+ EYE_HEIGHT_M`); (3) drive the camera there. The engine
//! only exposes a *delta*-based first-person control, so we feed it a
//! `FirstPersonInput` whose `move_local` carries the horizontal step **and** the
//! vertical correction `desired_y - current_y` in its Y component. A yaw rotation
//! about +Y leaves that Y component unchanged, so the camera lands exactly on the
//! surface; we mirror the engine's identical yaw-rotation of the horizontal step
//! to keep our `(x, z)` in lock-step with the engine's camera. The result: the
//! eye rises and falls with the terrain, ~1.7 m above it, with no clip or float.
//!
//! ## Per-vertex colour (first-person terrain)
//!
//! The walkable terrain mesh is coloured **per vertex** by biome + elevation (see
//! [`biome_terrain_color`]): the live backend shader
//! (`live_gpu_binding.rs::CUBE_WGSL`) now takes a per-vertex colour attribute and
//! multiplies it by the per-instance (material) colour, so the terrain material is
//! white and the per-vertex biome gradient shows true. Relief is still reinforced
//! by the shader's normal-based diffuse term (we compute correct per-vertex
//! normals). The 2D overworld map remains its own fully-coloured 2D-canvas path.

use std::cell::RefCell;
use std::rc::Rc;

use axiom::prelude::*;
use axiom_math::Quat;
use axiom_windowing::WindowingApi;
use wasm_bindgen::prelude::*;
use wasm_bindgen::JsCast;
use web_sys::{CanvasRenderingContext2d, HtmlCanvasElement, ImageData, KeyboardEvent, MouseEvent};

use crate::gameworld::sample_height_m;
use crate::geo;
use crate::model_world::{GameWorldLocalMap, CELL_SIZE_M, CHUNK_SIZE_CELLS};
use crate::presets::PlanetPreset;
use crate::sampler::{self, biome};
use crate::Growth;

/// The presentation canvas element id (must match `web/index.html`).
pub const CANVAS_ID: &str = "axiom-growth-canvas";
/// The overworld map canvas element id (must match `web/index.html`).
pub const MAP_CANVAS_ID: &str = "axiom-growth-map";

/// Surface size in physical pixels (the first-person WebGPU canvas).
const SURFACE_W: u32 = 960;
const SURFACE_H: u32 = 600;

/// Equirectangular overworld map size in pixels (2:1, lon × lat).
const MAP_W: u32 = 720;
const MAP_H: u32 = 360;

/// Half-extent of the sampled terrain, in metres, measured from the mesh
/// centre. The mesh spans `[centre - AREA_HALF_M, centre + AREA_HALF_M]` on each
/// axis (~320 m total) — kept small so a re-centre regenerates cheaply.
const AREA_HALF_M: f32 = 160.0;
/// Metres between terrain vertices. 1 m matches the worldgen cell size.
const STEP_M: f32 = 1.0;

/// Re-centre the streamed mesh once the player is more than this far (metres)
/// from the current mesh centre on either axis. Half the half-extent leaves a
/// generous margin of already-generated terrain ahead of the player in every
/// direction before the next regen, so the slide is never visible at the edge.
const RECENTER_THRESHOLD_M: f32 = AREA_HALF_M * 0.5;

/// Chunk side in metres (`CHUNK_SIZE_CELLS` cells × `CELL_SIZE_M`). The mesh
/// centre is snapped to this grid so re-centres land on chunk-aligned world
/// positions (the worldgen seams) and successive windows line up exactly.
const CHUNK_M: f32 = CHUNK_SIZE_CELLS as f32 * CELL_SIZE_M;

/// Eye height above the terrain surface (human ~1.7 m).
const EYE_HEIGHT_M: f32 = 1.7;

/// Metres walked per held movement tick, and radians turned per key tick.
const MOVE_SPEED: f32 = 0.6;
const TURN_SPEED: f32 = 0.03;
/// Radians of look per pixel of mouse movement.
const MOUSE_SENSITIVITY: f32 = 0.0025;

/// Log a line to the browser console, prefixed so the viewer is easy to spot.
fn log(msg: &str) {
    web_sys::console::log_1(&JsValue::from_str(&format!("[growth] {msg}")));
}

/// A linear colour channel from a known-finite literal.
fn ch(value: f32) -> Ratio {
    Ratio::new(value).expect("authored colour channel is finite")
}

// ===========================================================================
// Cross-call state (single-threaded wasm): the generated planet lives here
// between the JS-driven flow steps (generate → render map → pick → descend).
// ===========================================================================

thread_local! {
    /// The currently generated planet, set by [`generate`] and read by the
    /// overworld render + the pick/descend steps.
    static WORLD: RefCell<Option<Growth>> = const { RefCell::new(None) };
}

/// Parse the preset dropdown value into a [`PlanetPreset`] (defaults Earthlike).
fn parse_preset(name: &str) -> PlanetPreset {
    match name {
        "ocean" | "ocean_world" | "OceanWorld" => PlanetPreset::OceanWorld,
        "dry" | "Dry" => PlanetPreset::Dry,
        _ => PlanetPreset::Earthlike,
    }
}

// ===========================================================================
// (A) CONFIG → Generate, and (B) the overworld equirectangular map.
// ===========================================================================

/// Generate a planet from the config form and render the overworld map.
///
/// Called from JS on **Generate**. `sites` is the region-count / detail control.
/// The planet is stashed in [`WORLD`]; the equirectangular biome+elevation map is
/// drawn into the `#axiom-growth-map` 2D canvas, ready for the player to pick a
/// spot. Returns nothing — JS switches to the OVERWORLD screen on success.
#[wasm_bindgen]
pub fn generate(seed: &str, preset: &str, sites: u32) {
    console_error_panic_hook::set_once();
    let preset = parse_preset(preset);
    // Clamp the detail control to the worldgen-supported range (matches the
    // form slider; tiny values still produce a valid globe).
    let sites = sites.clamp(64, 2_600_000);
    log(&format!(
        "generate(): seed={seed:?} preset={} sites={sites}",
        preset.id()
    ));
    let growth = Growth::generate(seed, preset, sites);
    render_overworld_map(&growth);
    WORLD.with(|w| *w.borrow_mut() = Some(growth));
    log("generate(): overworld map drawn");
}

/// Draw an equirectangular biome+elevation map of the planet into the 2D map
/// canvas. Each pixel is a lat/long → unit direction → `sample_surface`, coloured
/// by biome with an elevation-driven shade so continents, coastlines, deserts,
/// forests and ice read at a glance. This is overworld option **(b1)** — the
/// robust, directly-clickable world-map path (see `BROWSER_VIEWER.md`).
fn render_overworld_map(growth: &Growth) {
    let document = web_sys::window()
        .expect("a browser window")
        .document()
        .expect("a document");
    let Some(canvas) = document.get_element_by_id(MAP_CANVAS_ID) else {
        log("map canvas missing; skipping overworld render");
        return;
    };
    let canvas: HtmlCanvasElement = canvas
        .dyn_into()
        .expect("the map element is a <canvas>");
    canvas.set_width(MAP_W);
    canvas.set_height(MAP_H);
    let ctx: CanvasRenderingContext2d = canvas
        .get_context("2d")
        .expect("2d context available")
        .expect("2d context present")
        .dyn_into()
        .expect("a 2d rendering context");

    // RGBA8 buffer, row-major top-to-bottom. y=0 is the north pole (lat +90).
    let mut rgba = vec![0u8; (MAP_W * MAP_H * 4) as usize];
    for py in 0..MAP_H {
        // Latitude from +pi/2 (top) to -pi/2 (bottom).
        let lat = std::f32::consts::FRAC_PI_2
            - (py as f32 + 0.5) / MAP_H as f32 * std::f32::consts::PI;
        for px in 0..MAP_W {
            // Longitude from -pi (left) to +pi (right).
            let lon = -std::f32::consts::PI
                + (px as f32 + 0.5) / MAP_W as f32 * std::f32::consts::TAU;
            let dir = geo::unit_dir_from_lat_lon(lat, lon);
            let s = sampler::sample_surface(&growth.atlas, dir);
            let [r, g, b] = biome_color(s.biome.0, s.elevation);
            let i = ((py * MAP_W + px) * 4) as usize;
            rgba[i] = r;
            rgba[i + 1] = g;
            rgba[i + 2] = b;
            rgba[i + 3] = 255;
        }
    }

    let clamped = wasm_bindgen::Clamped(&rgba[..]);
    let image = ImageData::new_with_u8_clamped_array_and_sh(clamped, MAP_W, MAP_H)
        .expect("image data dimensions are valid");
    ctx.put_image_data(&image, 0.0, 0.0)
        .expect("image data writes to the canvas");
}

/// A biome+elevation colour for a map pixel. Ocean shades from deep to shallow
/// blue with depth; land uses a per-biome base tinted brighter on high ground so
/// relief reads. Returns linear-ish sRGB bytes; exactness is not required (this
/// is a navigational map, not the deterministic worldgen output).
fn biome_color(biome_id: u32, elevation: f32) -> [u8; 3] {
    if elevation < 0.0 || biome_id == biome::OCEAN {
        // Deeper water is darker; clamp depth to a sane band.
        let depth = (-elevation).clamp(0.0, 1.0);
        let r = (20.0 - 10.0 * depth) as u8;
        let g = (70.0 - 30.0 * depth) as u8;
        let b = (150.0 - 40.0 * depth) as u8;
        return [r, g, b];
    }
    // Land: per-biome base, brightened with elevation so mountains pop.
    let base = match biome_id {
        biome::DESERT => [214.0, 196.0, 132.0],
        biome::RAINFOREST => [34.0, 120.0, 48.0],
        biome::TUNDRA => [180.0, 180.0, 190.0],
        biome::TAIGA => [60.0, 110.0, 80.0],
        _ => [90.0, 150.0, 80.0], // generic grassland fallback
    };
    let lift = (elevation.clamp(0.0, 1.5) / 1.5) * 60.0;
    [
        (base[0] + lift).min(255.0) as u8,
        (base[1] + lift).min(255.0) as u8,
        (base[2] + lift).min(255.0) as u8,
    ]
}

// ===========================================================================
// (C) SELECT — map a clicked pixel to a unit direction and test for land.
// ===========================================================================

/// Convert a normalised map click `(u, v)` in `[0,1]` (u = left→right, v =
/// top→bottom) to a unit direction on the planet. The inverse of the
/// equirectangular projection used by [`render_overworld_map`].
fn click_to_dir(u: f32, v: f32) -> axiom_math::Vec3 {
    let lat = std::f32::consts::FRAC_PI_2 - v.clamp(0.0, 1.0) * std::f32::consts::PI;
    let lon = -std::f32::consts::PI + u.clamp(0.0, 1.0) * std::f32::consts::TAU;
    geo::unit_dir_from_lat_lon(lat, lon)
}

/// Is the planet's surface at normalised map click `(u, v)` land (elevation ≥ 0)?
/// JS calls this to reject ocean picks with a hint before descending.
#[wasm_bindgen]
pub fn is_land(u: f32, v: f32) -> bool {
    WORLD.with(|w| {
        w.borrow().as_ref().is_some_and(|g| {
            let dir = click_to_dir(u, v);
            sampler::sample_surface(&g.atlas, dir).elevation >= 0.0
        })
    })
}

// ===========================================================================
// Terrain mesh build (shared by the descend step).
// ===========================================================================

/// A built terrain mesh window: the interleaved position+normal+colour vertex
/// stream the windowing backend uploads (10 floats/vertex) and its triangle
/// indices. Vertices are in **world** coordinates on x/z (so the mesh sits where
/// the camera's world position maps onto it) and recentred vertically by a
/// fixed global anchor height (so successive windows never jump in Y).
struct Terrain {
    vertices: Vec<f32>,
    indices: Vec<u32>,
}

/// A per-vertex biome+elevation colour for the walkable terrain, in **linear**
/// RGB `[0,1]` (the live shader works in linear space, and the terrain material
/// is white so this colour shows through unmultiplied). It mirrors the native
/// renderer's `color_for`/elevation ramp (`examples/render_maps.rs`): a per-biome
/// base, darkened in the low ground and blended toward snowy white on the high
/// ground, using the vertex's local relief `t` in `[0,1]` (0 = lowest sampled
/// vertex, 1 = highest). Ocean is depth-blue, though walkable terrain is land so
/// it is rarely hit. Returns `[r, g, b, 1.0]`.
fn biome_terrain_color(biome_id: u32, elevation: f32, moisture: f32, t: f32) -> [f32; 4] {
    if biome_id == biome::OCEAN || elevation < 0.0 {
        // Ocean: deeper (more negative elevation) is darker blue.
        let depth = (-elevation).clamp(0.0, 1.0);
        let shade = 1.0 - 0.6 * depth;
        return [0.05 * shade, 0.22 * shade, 0.55 * shade + 0.12, 1.0];
    }
    // Land: per-biome linear base (tan desert, green grass/rainforest, grey
    // tundra, dark-green taiga), matching the native ramp's intent.
    let base = match biome_id {
        x if x == biome::DESERT => [0.82, 0.70, 0.43],
        x if x == biome::RAINFOREST => [0.12, 0.47, 0.16],
        x if x == biome::TUNDRA => [0.66, 0.66, 0.63],
        x if x == biome::TAIGA => [0.16, 0.35, 0.27],
        _ => [0.35, 0.55, 0.27], // generic grassland
    };
    let t = t.clamp(0.0, 1.0);
    let snow = ((t - 0.62).max(0.0) / 0.38).clamp(0.0, 1.0); // blend to white up high
    let dry = 1.0 - 0.2 * moisture.clamp(0.0, 1.0);
    // Darken the low ground, lighten toward the peaks, then blend in snow.
    let shade = (0.55 + 0.45 * t) * dry;
    let mix = |c: f32| c * shade * (1.0 - snow) + 0.96 * snow;
    [mix(base[0]), mix(base[1]), mix(base[2]), 1.0]
}

/// The **true** surface height (metres, absolute) at a world position. This is
/// the fixed global vertical reference: it is sampled ONCE at the descent spot
/// and every streamed mesh is recentred by this same value, so re-centred
/// windows line up vertically (no jump) while staying pure functions of world
/// position (so they line up horizontally too — the seam stays consistent with
/// the original static mesh).
fn anchor_height_m(growth: &Growth, localmap: &GameWorldLocalMap, cx_m: f32, cz_m: f32) -> f32 {
    sample_height_m(&growth.atlas, localmap, growth.seed.value, cx_m, cz_m)
}

/// Sample the planet's terrain into a flat grid mesh centred on `(cx_m, cz_m)`
/// in world metres.
///
/// Vertices are emitted in **world** coordinates on x/z — each vertex samples
/// the absolute world position `centre + offset` — and recentred vertically by
/// the fixed global `anchor_h` (so the walkable surface sits near y = 0 and
/// successive windows never jump in Y; `anchor_h` is sampled once and reused for
/// every regen, never re-derived per mesh). Per-vertex normals come from the
/// height field via central differences, and each vertex carries a
/// biome+elevation colour (see [`biome_terrain_color`]). Because heights are a
/// pure function of world position recentred by a constant, a window centred at
/// any chunk-aligned spot is seam-consistent with the original.
fn build_terrain(
    growth: &Growth,
    localmap: &GameWorldLocalMap,
    cx_m: f32,
    cz_m: f32,
    anchor_h: f32,
) -> Terrain {
    let seed = growth.seed.value;
    let atlas = &growth.atlas;

    let side: usize = ((AREA_HALF_M * 2.0) / STEP_M) as usize + 1;
    let origin_x = cx_m - AREA_HALF_M;
    let origin_z = cz_m - AREA_HALF_M;

    let raw: Vec<f32> = (0..side * side)
        .map(|i| {
            let gx = i % side;
            let gz = i / side;
            let x_m = origin_x + gx as f32 * STEP_M;
            let z_m = origin_z + gz as f32 * STEP_M;
            sample_height_m(atlas, localmap, seed, x_m, z_m)
        })
        .collect();

    let height_at = |gx: usize, gz: usize| -> f32 { raw[gz * side + gx] - anchor_h };

    // Local relief range (recentred), used to grade vertex colour from low
    // (dark) to high (snow). Guard against a flat mesh with a min span.
    let (lo, hi) = raw.iter().fold((f32::INFINITY, f32::NEG_INFINITY), |(lo, hi), &h| {
        (lo.min(h - anchor_h), hi.max(h - anchor_h))
    });
    let span = (hi - lo).max(1.0);

    let mut vertices: Vec<f32> = Vec::with_capacity(side * side * 10);
    (0..side).for_each(|gz| {
        (0..side).for_each(|gx| {
            let x_m = origin_x + gx as f32 * STEP_M;
            let z_m = origin_z + gz as f32 * STEP_M;
            let y_m = height_at(gx, gz);

            let xl = gx.saturating_sub(1);
            let xr = (gx + 1).min(side - 1);
            let zl = gz.saturating_sub(1);
            let zr = (gz + 1).min(side - 1);
            let dhx = height_at(xr, gz) - height_at(xl, gz);
            let dhz = height_at(gx, zr) - height_at(gx, zl);
            let dx = (xr - xl) as f32 * STEP_M;
            let dz = (zr - zl) as f32 * STEP_M;
            let nx = -dhx * dz;
            let ny = dx * dz;
            let nz = -dhz * dx;
            let len = (nx * nx + ny * ny + nz * nz).sqrt().max(1.0e-6);

            // Biome + elevation colour at this vertex's world position. The unit
            // direction is the same mapping the height sampler uses, so colour and
            // relief agree. `t` is the vertex's local relief in [0,1].
            let dir = localmap.world_metres_to_unit_dir(x_m, z_m);
            let s = sampler::sample_surface(atlas, Vec3::new(dir[0], dir[1], dir[2]));
            let t = (y_m - lo) / span;
            let [cr, cg, cb, ca] = biome_terrain_color(s.biome.0, s.elevation, s.moisture, t);

            vertices.extend_from_slice(&[
                x_m,
                y_m,
                z_m,
                nx / len,
                ny / len,
                nz / len,
                cr,
                cg,
                cb,
                ca,
            ]);
        });
    });

    let mut indices: Vec<u32> = Vec::with_capacity((side - 1) * (side - 1) * 6);
    (0..side - 1).for_each(|gz| {
        (0..side - 1).for_each(|gx| {
            let i0 = (gz * side + gx) as u32;
            let i1 = i0 + 1;
            let i2 = ((gz + 1) * side + gx) as u32;
            let i3 = i2 + 1;
            indices.extend_from_slice(&[i0, i2, i1, i1, i2, i3]);
        });
    });

    Terrain { vertices, indices }
}

/// Author the engine scene: a single identity-transform terrain renderable (one
/// draw, whose MVP becomes the camera view-projection), a first-person camera at
/// eye height, and a directional light. The terrain geometry is uploaded — and
/// re-uploaded as the player walks — separately through `run_web_streaming`;
/// this renderable exists so the engine produces the one MVP the terrain needs.
/// The camera starts at `(0, eye_y, 0)` facing -Z; the per-frame loop drives it
/// across the surface.
fn build_viewer_app(eye_y: f32) -> RunningApp {
    let clear = Color::linear_rgb(ch(0.45), ch(0.62), ch(0.85)); // sky
    App::new()
        .window(
            Window::new(SURFACE_W, SURFACE_H)
                .with_surface_id(CANVAS_ID)
                .with_clear_color(clear),
        )
        .add_plugins(DefaultPlugins)
        .setup(move |world, meshes, materials| {
            let mesh = meshes.add(Mesh::cube());
            // White material: the live shader multiplies the per-instance
            // (material) colour by the per-vertex colour, so a white instance lets
            // the terrain's per-vertex biome colours show true.
            let material = materials.add(Material::lit(Color::WHITE));
            world.spawn((Transform::IDENTITY, Renderable { mesh, material }));
            world.spawn((
                Transform::from_translation(Vec3::new(0.0, eye_y, 0.0)),
                Camera::perspective(PerspectiveProjection {
                    fov_y: Angle::degrees(70.0),
                    near: Meters::new(0.1).expect("near plane is finite"),
                    far: Meters::new(2000.0).expect("far plane is finite"),
                }),
                Controller::new(0),
            ));
            world.spawn((
                Transform::IDENTITY,
                DirectionalLight {
                    direction: Vec3::new(0.35, -1.0, 0.25),
                    color: Color::WHITE,
                    intensity: ch(1.0),
                },
            ));
        })
        .build()
}

// ===========================================================================
// (D) DESCEND — build the local world at the picked spot and walk it.
// ===========================================================================

/// The walking player's authoritative state, owned by the app (NOT the engine's
/// free-fly controller). The engine integrates horizontal movement and look; we
/// own these so we can sample the ground under `(x, z)` and seat the camera on
/// it. `engine_y` mirrors the camera's current engine-space Y so we can feed the
/// exact vertical correction each frame.
#[derive(Clone, Copy)]
struct Player {
    x: f32,
    z: f32,
    yaw: f32,
    pitch: f32,
    engine_y: f32,
}

/// Held movement/turn keys, polled each frame.
#[derive(Default, Clone, Copy)]
struct Keys {
    forward: bool,
    backward: bool,
    strafe_left: bool,
    strafe_right: bool,
    turn_left: bool,
    turn_right: bool,
}

/// Mouse-look deltas accumulated between frames (radians), drained each tick.
#[derive(Default, Clone, Copy)]
struct Look {
    yaw: f32,
    pitch: f32,
}

/// Descend into the first-person walkable world at the clicked map spot.
///
/// `(u, v)` is the normalised map click (u left→right, v top→bottom). The spot's
/// unit direction anchors a local tangent frame via
/// [`GameWorldLocalMap::anchored_at`]; the terrain mesh is built around it and
/// the first-person windowing loop starts. The ground-follow integration
/// (see the module docs) keeps the eye ~1.7 m above the surface as the player
/// walks over hills. JS calls this after a successful land [`is_land`] check.
#[wasm_bindgen]
pub fn descend(u: f32, v: f32) {
    console_error_panic_hook::set_once();
    let dir = click_to_dir(u, v);
    log(&format!(
        "descend(): u={u:.3} v={v:.3} dir=({:.3},{:.3},{:.3})",
        dir.x, dir.y, dir.z
    ));

    // Take the generated world out of the cell (we keep it inside the closure).
    let growth = match WORLD.with(|w| w.borrow_mut().take()) {
        Some(g) => g,
        None => {
            log("descend(): no generated world — call generate() first");
            return;
        }
    };

    let localmap = GameWorldLocalMap::anchored_at(&growth.atlas, dir);

    // The fixed global vertical reference: the true surface height at the descent
    // spot (world origin). EVERY mesh — the first and every re-centred one — is
    // recentred by THIS value, so regenerated windows never jump in Y. It is
    // sampled once here and never re-derived per regen.
    let anchor_h = anchor_height_m(&growth, &localmap, 0.0, 0.0);

    // The first mesh window is centred on the descent spot (world origin).
    let terrain = build_terrain(&growth, &localmap, 0.0, 0.0, anchor_h);
    log(&format!(
        "terrain built: {} vertices, {} indices, anchor_h={:.1} m",
        terrain.vertices.len() / 10,
        terrain.indices.len(),
        anchor_h
    ));

    // The player starts at the anchor (mesh origin), eye at recentred ground 0.
    let start_ground = 0.0; // mesh is recentred so the anchor surface is y = 0.
    let eye_y = start_ground + EYE_HEIGHT_M;
    let mut running = build_viewer_app(eye_y);

    let seed = growth.seed.value;

    let player = Rc::new(RefCell::new(Player {
        x: 0.0,
        z: 0.0,
        yaw: 0.0,
        pitch: 0.0,
        engine_y: eye_y,
    }));

    // The streamed mesh's current centre, in chunk-aligned world metres. Starts
    // on the descent spot; slides to the player's chunk on each threshold cross.
    let mut mesh_cx: f32 = 0.0;
    let mut mesh_cz: f32 = 0.0;

    // Input capture.
    let keys = Rc::new(RefCell::new(Keys::default()));
    install_key_listener(&keys, "keydown", true);
    install_key_listener(&keys, "keyup", false);
    let look = Rc::new(RefCell::new(Look::default()));
    install_pointer_lock();
    install_mouse_look(&look);

    let mut windowing = WindowingApi::new();
    windowing
        .configure_surface(SURFACE_W, SURFACE_H)
        .expect("surface dimensions are valid");

    let mut tick: u64 = 0;
    let _ = windowing.run_web_streaming(
        CANVAS_ID,
        terrain.vertices,
        terrain.indices,
        1, // one renderable (the terrain) -> one instance
        move |_raf_tick| {
            let k = *keys.borrow();
            let (look_yaw, look_pitch) = {
                let mut l = look.borrow_mut();
                let v = (l.yaw, l.pitch);
                *l = Look::default();
                v
            };

            // --- 1. Update look (yaw/pitch) from keys + mouse. ---
            let key_yaw = (k.turn_left as i32 - k.turn_right as i32) as f32 * TURN_SPEED;
            let yaw_delta = key_yaw + look_yaw;
            let pitch_delta = look_pitch;

            let mut p = player.borrow_mut();
            p.yaw += yaw_delta;
            // The engine clamps pitch internally; mirror a sane clamp so our
            // tracked pitch matches the engine's accumulated value.
            p.pitch = (p.pitch + pitch_delta).clamp(-1.5, 1.5);

            // --- 2. Integrate horizontal movement, exactly as the engine will. ---
            // Local frame: -Z forward, +X right. The engine yaw-rotates this
            // (yaw-only, so it stays horizontal) and adds it to the camera. We
            // mirror that same rotation into our (x, z) so we stay in lock-step.
            let forward = (k.forward as i32 - k.backward as i32) as f32 * MOVE_SPEED;
            let strafe = (k.strafe_right as i32 - k.strafe_left as i32) as f32 * MOVE_SPEED;
            let move_local = Vec3::new(strafe, 0.0, -forward);
            let yh = p.yaw * 0.5;
            let yaw_q = Quat::new(0.0, yh.sin(), 0.0, yh.cos());
            let world_step = yaw_q.rotate(move_local);
            p.x += world_step.x;
            p.z += world_step.z;

            // --- 3. Sample the ground under the new (x, z) and seat the eye. ---
            // sample_height_m is absolute; the mesh is recentred by anchor_h, so
            // the mesh-space surface is (sampled - anchor_h), eye is + EYE_HEIGHT.
            let sampled = sample_height_m(&growth.atlas, &localmap, seed, p.x, p.z);
            let desired_y = sampled - anchor_h + EYE_HEIGHT_M;

            // --- 4. Drive the camera: horizontal step + vertical correction. ---
            // The engine's controller does translation += yaw.rotate(move_local).
            // A +Y yaw rotation preserves the Y component, so a move_local.y of
            // (desired_y - engine_y) lands the camera exactly on the surface.
            let dy = desired_y - p.engine_y;
            p.engine_y = desired_y;
            let control = FirstPersonInput::new(
                0,
                Vec3::new(strafe, dy, -forward),
                Angle::radians(yaw_delta),
                Angle::radians(pitch_delta),
            );
            let player_x = p.x;
            let player_z = p.z;
            drop(p);

            let outcome = running.tick_with_controls(tick, &[], &[control]);
            tick += 1;

            // --- 5. Stream the terrain: re-centre the mesh once the player has
            // walked past the threshold from the current mesh centre. The new
            // centre is snapped to the chunk grid (the worldgen seams), so the
            // re-centred window lines up exactly with the old one and with the
            // original static mesh — heights are pure functions of world position
            // recentred by the SAME fixed anchor, so there is no horizontal seam
            // and no vertical jump. The camera/player are untouched (they live in
            // continuous world space); only the uploaded mesh window slides. We
            // regen at most once per frame, and only on a threshold cross. ---
            let crossed = (player_x - mesh_cx).abs() >= RECENTER_THRESHOLD_M
                || (player_z - mesh_cz).abs() >= RECENTER_THRESHOLD_M;
            let new_geometry = crossed.then(|| {
                mesh_cx = (player_x / CHUNK_M).round() * CHUNK_M;
                mesh_cz = (player_z / CHUNK_M).round() * CHUNK_M;
                let t = build_terrain(&growth, &localmap, mesh_cx, mesh_cz, anchor_h);
                (t.vertices, t.indices)
            });

            (
                outcome.clear_color(),
                outcome.instance_floats(),
                outcome.draws().len() as u32,
                new_geometry,
            )
        },
    );
}

// ===========================================================================
// Input plumbing (shared with the first-person view).
// ===========================================================================

/// The presentation canvas element (the first-person WebGPU canvas).
fn canvas() -> web_sys::Element {
    web_sys::window()
        .expect("a browser window")
        .document()
        .expect("a document")
        .get_element_by_id(CANVAS_ID)
        .expect("the growth canvas is in the page")
}

/// Is the pointer currently locked?
fn pointer_is_locked() -> bool {
    web_sys::window()
        .and_then(|w| w.document())
        .and_then(|d| d.pointer_lock_element())
        .is_some()
}

/// Map held keys into the shared key set. Matches on `key` so WASD + arrows work.
fn install_key_listener(keys: &Rc<RefCell<Keys>>, event: &str, pressed: bool) {
    let keys = keys.clone();
    let callback = Closure::<dyn FnMut(KeyboardEvent)>::new(move |e: KeyboardEvent| {
        let mut k = keys.borrow_mut();
        match e.key().as_str() {
            "w" | "W" | "ArrowUp" => k.forward = pressed,
            "s" | "S" | "ArrowDown" => k.backward = pressed,
            "a" | "A" => k.strafe_left = pressed,
            "d" | "D" => k.strafe_right = pressed,
            "ArrowLeft" => k.turn_left = pressed,
            "ArrowRight" => k.turn_right = pressed,
            _ => return,
        }
        e.prevent_default();
    });
    web_sys::window()
        .expect("a browser window")
        .add_event_listener_with_callback(event, callback.as_ref().unchecked_ref())
        .expect("key listener installs");
    callback.forget();
}

/// Capture the pointer when the canvas is clicked (classic FPS mouse-look).
fn install_pointer_lock() {
    let canvas = canvas();
    let target = canvas.clone();
    let cb = Closure::<dyn FnMut(MouseEvent)>::new(move |_e: MouseEvent| {
        let _ = target.request_pointer_lock();
    });
    canvas
        .add_event_listener_with_callback("click", cb.as_ref().unchecked_ref())
        .expect("click listener installs");
    cb.forget();
}

/// Accumulate relative mouse movement into yaw/pitch while the pointer is locked.
/// Mouse right turns right (−yaw); mouse up looks up (+pitch).
fn install_mouse_look(look: &Rc<RefCell<Look>>) {
    let look = look.clone();
    let cb = Closure::<dyn FnMut(MouseEvent)>::new(move |e: MouseEvent| {
        if !pointer_is_locked() {
            return;
        }
        let mut l = look.borrow_mut();
        l.yaw += -(e.movement_x() as f32) * MOUSE_SENSITIVITY;
        l.pitch += -(e.movement_y() as f32) * MOUSE_SENSITIVITY;
    });
    web_sys::window()
        .expect("a browser window")
        .add_event_listener_with_callback("mousemove", cb.as_ref().unchecked_ref())
        .expect("mousemove listener installs");
    cb.forget();
}
