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
use std::collections::HashMap;
use std::rc::Rc;

use axiom::prelude::*;
use axiom_math::Quat;
use axiom_windowing::WindowingApi;
use wasm_bindgen::prelude::*;
use wasm_bindgen::JsCast;
use web_sys::{CanvasRenderingContext2d, HtmlCanvasElement, ImageData, KeyboardEvent, MouseEvent};

use crate::gameworld::{sample_height_m, sample_height_m_lod};
use crate::geo;
use crate::model_world::{GameWorldLocalMap, CELL_SIZE_M, CHUNK_SIZE_CELLS, CHUNK_VERT_SIDE};
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

/// Metres between terrain vertices. 1 m matches the worldgen cell size.
const STEP_M: f32 = 1.0;

/// Chunk side in metres (`CHUNK_SIZE_CELLS` cells × `CELL_SIZE_M`). Terrain is
/// streamed as a grid of these chunks; the player's chunk is `floor(p / CHUNK_M)`
/// and chunk `(cx, cz)`'s world origin is `(cx*CHUNK_M, cz*CHUNK_M)`. Chunk
/// borders fall on the worldgen seams, so neighbouring chunks line up exactly.
const CHUNK_M: f32 = CHUNK_SIZE_CELLS as f32 * CELL_SIZE_M;

/// Vertical span (metres, recentred) over which the per-vertex colour grades
/// from low ground (dark) to snowy peaks, used as `t = 0.5 + y/RELIEF_SPAN_M`
/// clamped to `[0,1]`. A **fixed** regional scale (not a per-chunk min/max) so
/// the colour at a shared chunk edge is identical on both sides — no colour seam
/// — and the whole landscape shares one elevation gradient. ~120 m roughly
/// matches the relief a walkable region spans (hills ±55 m plus mountain flank).
const RELIEF_SPAN_M: f32 = 120.0;

/// Floats per terrain vertex: position(3) + normal(3) + biome colour(4).
const VERT_FLOATS: usize = 12;

/// World-space size, in metres, of one biome-atlas cell tile on the terrain. The
/// biome's atlas cell repeats every `BIOME_TILE_M` metres so the surface texture
/// reads as a fine-grained material rather than one stretched swatch.
const BIOME_TILE_M: f32 = 3.0;
/// Vertices a single chunk emits (`CHUNK_VERT_SIDE`² = 17² = 289). Adjacent
/// chunks duplicate their shared edge vertices, which is harmless because
/// identical world positions/normals/colours produce no visible seam.
const CHUNK_VERTS: usize = CHUNK_VERT_SIDE * CHUNK_VERT_SIDE;

/// Cap on how many chunks may be (re)generated in a single frame, so the
/// per-frame terrain cost is bounded (a few × the single-chunk cost) and there
/// is no all-at-once edge-cross spike. The backlog drains over the next frames
/// as the player keeps walking. A coarse far chunk costs about the same as a
/// near one (same 289 vertices), so one cap fits all LODs.
const MAX_GEN_PER_FRAME: usize = 6;

// ===========================================================================
// Level-of-detail (LOD) terrain — concentric clipmap-style rings.
//
// Every chunk still emits `CHUNK_VERT_SIDE`²(=289) vertices. A chunk at **LOD L**
// has world size `CHUNK_M * 2^L` m and vertex spacing `STEP_M * 2^L` m, so a far
// chunk covers 2^L× more ground for the same vertex cost. LOD is assigned by
// distance: LOD 0 is a full-detail square of half-extent `lod0_radius_chunks`
// around the player; each coarser ring `L` is a band `RING_BAND_CHUNKS` chunks
// wide (in its OWN chunk size) beyond the finer coverage, so the outer radius
// roughly doubles per level and the draw distance grows fast for little cost.
//
// LOD is **visual only**: ground-follow / collision always use full-detail
// `sample_height_m` (LOD 0 quality), so render-LOD never moves where the player
// walks. Cracks where a coarse chunk meets a fine neighbour are hidden by
// per-chunk vertical SKIRTS (the border ring of verts dropped straight down).
// ===========================================================================

/// World size (metres) of a chunk at LOD `lod`: the base `CHUNK_M` doubled per
/// level. LOD 0 = 16 m (1 m spacing); LOD 3 = 128 m (8 m spacing); etc.
fn chunk_size_m(lod: u8) -> f32 {
    CHUNK_M * (1u32 << lod) as f32
}

/// Vertex spacing (metres) within a LOD-`lod` chunk: `STEP_M * 2^lod`. This is
/// also the `min_feature_m` the chunk passes to `sample_height_m_lod`, so each
/// coarse chunk omits the sub-vertex detail octaves it could only alias.
fn lod_spacing_m(lod: u8) -> f32 {
    STEP_M * (1u32 << lod) as f32
}

/// Band width, in a ring's OWN LOD chunks, of each coarse ring beyond the finer
/// coverage. Wider = smoother LOD progression but more chunks. 3 keeps the ring
/// vertex count modest while still doubling the outer radius each level.
const RING_BAND_CHUNKS: i32 = 3;

/// Skirt depth as a multiple of the chunk's vertex spacing. The border ring of
/// every chunk is dropped this many spacings straight down, forming a vertical
/// curtain that plugs the gap a coarse neighbour's sparser edge would otherwise
/// leave. Scaling by spacing keeps far (coarse) skirts deep enough to cover the
/// larger inter-LOD height error without an over-deep skirt up close.
const SKIRT_DEPTH_SPACINGS: f32 = 8.0;

/// Tunable terrain LOD configuration, read once at [`descend`] time from the
/// values JS pushed via [`set_terrain_config`] (or the defaults below).
#[derive(Clone, Copy, Debug)]
struct TerrainConfig {
    /// Number of LOD levels (rings), `>= 1`. The "view distance" control: more
    /// levels ⇒ exponentially larger outer radius. LOD 0..lod_levels-1 exist.
    lod_levels: u8,
    /// Half-extent, in LOD-0 chunks, of the full-detail near square (R0). The
    /// "quality" control's main knob: bigger ⇒ full 1 m detail reaches further
    /// before the first coarsening; aggressive/Low ⇒ small.
    lod0_radius_chunks: i32,
    /// Extra detail-cap bias (metres) added on top of each chunk's vertex
    /// spacing when sampling far rings. 0 = drop only octaves finer than the
    /// spacing (crispest); larger ⇒ drop coarser octaves too (cheaper, flatter
    /// far terrain). The "quality" control's aggressiveness knob.
    detail_bias_m: f32,
}

impl Default for TerrainConfig {
    fn default() -> Self {
        // Medium quality / ~700 m view by default: 5 rings, full detail out to
        // 4 chunks (64 m), no extra detail bias.
        Self {
            lod_levels: 5,
            lod0_radius_chunks: 4,
            detail_bias_m: 0.0,
        }
    }
}

impl TerrainConfig {
    /// The detail cap (`min_feature_m`) a LOD-`lod` render chunk samples with.
    /// LOD 0 is forced to `0.0` (full detail) so the near ring matches collision
    /// bit-for-bit; coarser rings cap at their spacing plus the quality bias.
    fn min_feature_m(&self, lod: u8) -> f32 {
        if lod == 0 {
            0.0
        } else {
            lod_spacing_m(lod) + self.detail_bias_m
        }
    }

    /// Outer radius (metres, Chebyshev half-extent) covered up to and including
    /// LOD `lod`. LOD 0 reaches `lod0_radius_chunks` LOD-0 chunks; each coarser
    /// ring adds `RING_BAND_CHUNKS` of its own (doubling) chunk size, so the
    /// radius grows ~geometrically. Used both to build rings and to report the
    /// draw distance.
    fn outer_radius_m(&self, lod: u8) -> f32 {
        let mut r = self.lod0_radius_chunks as f32 * chunk_size_m(0);
        let mut l = 1u8;
        while l <= lod {
            r += RING_BAND_CHUNKS as f32 * chunk_size_m(l);
            l += 1;
        }
        r
    }

    /// The whole terrain's approximate draw distance in metres (outer radius of
    /// the coarsest ring).
    fn draw_distance_m(&self) -> f32 {
        self.outer_radius_m(self.lod_levels.saturating_sub(1))
    }
}

thread_local! {
    /// Terrain LOD config, optionally overridden by JS via [`set_terrain_config`]
    /// before [`descend`]. Read once at descent; later edits need a fresh descent.
    static TERRAIN_CONFIG: RefCell<TerrainConfig> = const {
        RefCell::new(TerrainConfig { lod_levels: 5, lod0_radius_chunks: 4, detail_bias_m: 0.0 })
    };
}

/// Configure the distance-LOD terrain BEFORE calling [`descend`]. Called from JS
/// when the player presses descend, reading the config-screen controls.
///
/// * `lod_levels` — number of concentric LOD rings (clamped `1..=10`). The view
///   distance: each level roughly doubles the outer radius.
/// * `lod0_radius_chunks` — half-extent in 16 m chunks of the full-detail near
///   square (clamped `1..=24`). The quality: how far crisp 1 m terrain reaches.
/// * `detail_bias_m` — extra metres added to each far ring's detail cap (clamped
///   `0..=64`). Aggressive quality drops more octaves far out for cheaper frames.
///
/// The estimate the UI shows should match [`TerrainConfig::draw_distance_m`].
#[wasm_bindgen]
pub fn set_terrain_config(lod_levels: u32, lod0_radius_chunks: u32, detail_bias_m: f32) {
    let cfg = TerrainConfig {
        lod_levels: lod_levels.clamp(1, 10) as u8,
        lod0_radius_chunks: lod0_radius_chunks.clamp(1, 24) as i32,
        detail_bias_m: detail_bias_m.clamp(0.0, 64.0),
    };
    log(&format!(
        "set_terrain_config(): levels={} lod0_radius={} bias={:.0}m → draw≈{:.0}m",
        cfg.lod_levels,
        cfg.lod0_radius_chunks,
        cfg.detail_bias_m,
        cfg.draw_distance_m()
    ));
    TERRAIN_CONFIG.with(|c| *c.borrow_mut() = cfg);
}

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
    let canvas: HtmlCanvasElement = canvas.dyn_into().expect("the map element is a <canvas>");
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
        let lat =
            std::f32::consts::FRAC_PI_2 - (py as f32 + 0.5) / MAP_H as f32 * std::f32::consts::PI;
        for px in 0..MAP_W {
            // Longitude from -pi (left) to +pi (right).
            let lon =
                -std::f32::consts::PI + (px as f32 + 0.5) / MAP_W as f32 * std::f32::consts::TAU;
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

/// A loaded chunk's key: grid coord `(cx, cz)` **in its own LOD's chunk size**
/// plus the `lod` level. World origin = `(cx, cz) * chunk_size_m(lod)`. Two
/// chunks at different LODs occupy different keys even over the same ground.
type ChunkKey = (i32, i32, u8);

/// Skirt vertices per chunk: one dropped twin per border surface vertex (the
/// perimeter of the `CHUNK_VERT_SIDE` grid), used to plug LOD-boundary cracks.
const SKIRT_VERTS: usize = 4 * (CHUNK_VERT_SIDE - 1); // 4*16 = 64
/// Total vertices a chunk emits at ANY LOD: the 289 surface verts plus the 64
/// skirt verts. Constant across LODs (a far chunk just covers more ground), so a
/// chunk's base offset in the combined buffer is `index * VERTS_PER_CHUNK`.
const VERTS_PER_CHUNK: usize = CHUNK_VERTS + SKIRT_VERTS; // 289 + 64 = 353

/// One generated chunk's geometry: an interleaved vertex stream (10 floats each:
/// position, normal, biome colour) and the chunk-local triangle indices (surface
/// grid + skirt), to be offset by the chunk's base vertex at assembly.
struct ChunkMesh {
    vertices: Vec<f32>,
    /// Local indices in `0..VERTS_PER_CHUNK`; assembly adds the per-chunk base.
    indices: Vec<u32>,
}

/// Generate ONE chunk's terrain geometry at the given `lod`, in **world**
/// coordinates, recentred vertically by the fixed global `anchor_h`.
///
/// A LOD-`lod` chunk is `chunk_size_m(lod)` metres square with vertex spacing
/// `lod_spacing_m(lod)`; chunk `(chunk_x, chunk_z)`'s world origin is
/// `(chunk_x, chunk_z) * chunk_size_m(lod)`. It carries `CHUNK_VERT_SIDE`²(=289)
/// **surface** vertices on that spacing — so a coarse chunk covers 2^lod× more
/// ground for the same vertex cost — plus a ring of **skirt** vertices.
///
/// **LOD-aware sampling.** Heights use `sample_height_m_lod(.., min_feature_m)`,
/// where LOD 0 passes `0.0` (full detail, bit-identical to collision's
/// `sample_height_m`) and coarser LODs pass their spacing-derived cap, dropping
/// sub-vertex octaves they could only alias. Heights are sampled on a `(side+2)`
/// **apron** at the LOD spacing so every surface vertex (edges included) gets a
/// central-difference normal from real neighbours; since the sampler is a pure
/// function of world position, two chunks **at the same LOD** share edge heights
/// and normals exactly (no seam within a ring).
///
/// **Skirts.** Each of the 64 border surface verts gets a twin at the same X/Z
/// but Y dropped by `SKIRT_DEPTH_SPACINGS * spacing`. Quads stitch each border
/// edge to its skirt twins, forming a vertical curtain (same colour) that hides
/// the gap a coarser neighbour's sparser edge would otherwise leave at a LOD
/// boundary. Y is recentred by the **passed-in** `anchor_h` (never re-derived).
fn gen_chunk(
    growth: &Growth,
    localmap: &GameWorldLocalMap,
    seed: u64,
    cfg: &TerrainConfig,
    chunk_x: i32,
    chunk_z: i32,
    lod: u8,
    anchor_h: f32,
) -> ChunkMesh {
    let atlas = &growth.atlas;
    let size_m = chunk_size_m(lod);
    let spacing = lod_spacing_m(lod);
    let min_feature_m = cfg.min_feature_m(lod);
    let origin_x = chunk_x as f32 * size_m;
    let origin_z = chunk_z as f32 * size_m;

    // (side+2)×(side+2) apron of LOD-sampled heights: apron index a in 0..APRON
    // maps to surface offset (a-1)*spacing, so a = 1..=side is the 17×17 surface
    // and a = 0 / a = APRON-1 are the neighbour apron feeding the edge normals.
    const APRON: usize = CHUNK_VERT_SIDE + 2; // 19
    let raw: Vec<f32> = (0..APRON * APRON)
        .map(|i| {
            let ax = i % APRON;
            let az = i / APRON;
            let x_m = origin_x + (ax as f32 - 1.0) * spacing;
            let z_m = origin_z + (az as f32 - 1.0) * spacing;
            sample_height_m_lod(atlas, localmap, seed, x_m, z_m, min_feature_m)
        })
        .collect();

    // Recentred surface height at surface grid coord (sx, sz) in 0..side. The
    // apron index is (sx+1, sz+1), so the apron's extra ring gives every surface
    // vertex — edges included — a real neighbour on each side for its normal.
    let apron_at = |ax: usize, az: usize| -> f32 { raw[az * APRON + ax] - anchor_h };
    let height_at = |sx: usize, sz: usize| -> f32 { apron_at(sx + 1, sz + 1) };

    // Emit one surface vertex (position, normal, biome colour). `drop` lowers Y
    // by a skirt depth and is used for the skirt twins.
    let emit = |out: &mut Vec<f32>, sx: usize, sz: usize, drop: f32| {
        let x_m = origin_x + sx as f32 * spacing;
        let z_m = origin_z + sz as f32 * spacing;
        let y_m = height_at(sx, sz);

        // Central differences over the apron (surface (sx,sz) is apron (sx+1,sz+1),
        // so its four neighbours are apron sx..sx+2 / sz..sz+2 — all in range).
        let dhx = apron_at(sx + 2, sz + 1) - apron_at(sx, sz + 1);
        let dhz = apron_at(sx + 1, sz + 2) - apron_at(sx + 1, sz);
        let dx = 2.0 * spacing;
        let dz = 2.0 * spacing;
        let nx = -dhx * dz;
        let ny = dx * dz;
        let nz = -dhz * dx;
        let len = (nx * nx + ny * ny + nz * nz).sqrt().max(1.0e-6);

        // Biome + elevation colour at this vertex's world position (same mapping
        // the height sampler uses, so colour and relief agree). `t` grades low
        // (dark) → high (snow) against the fixed regional relief span. The skirt
        // twin reuses the SAME colour so the curtain blends with the surface.
        let dir = localmap.world_metres_to_unit_dir(x_m, z_m);
        let s = sampler::sample_surface(atlas, Vec3::new(dir[0], dir[1], dir[2]));
        let t = (0.5 + y_m / RELIEF_SPAN_M).clamp(0.0, 1.0);
        let [cr, cg, cb, ca] = biome_terrain_color(s.biome.0, s.elevation, s.moisture, t);

        // Albedo UV into the biome atlas: pick the vertex's biome cell, then tile
        // a fractional position within that 0.5×0.5 cell by world metres so the
        // surface texture repeats across the ground. The per-vertex biome colour
        // above still tints the sampled albedo, so biomes mapping to the same
        // atlas cell stay colour-distinct.
        let (cell_u, cell_v) = Texture::biome_cell_origin(s.biome.0);
        let u = cell_u + (x_m / BIOME_TILE_M).rem_euclid(1.0) * 0.5;
        let v = cell_v + (z_m / BIOME_TILE_M).rem_euclid(1.0) * 0.5;

        out.extend_from_slice(&[
            x_m,
            y_m - drop,
            z_m,
            nx / len,
            ny / len,
            nz / len,
            u,
            v,
            cr,
            cg,
            cb,
            ca,
        ]);
    };

    let side = CHUNK_VERT_SIDE;
    let mut vertices: Vec<f32> = Vec::with_capacity(VERTS_PER_CHUNK * VERT_FLOATS);

    // --- Surface verts: the 17×17 grid (indices 0..CHUNK_VERTS), row-major. ---
    (0..side).for_each(|sz| (0..side).for_each(|sx| emit(&mut vertices, sx, sz, 0.0)));

    // --- Skirt verts: one dropped twin per border surface vert, appended in a
    // fixed perimeter order (top row, bottom row, left col, right col interiors)
    // so `skirt_index` below addresses them deterministically. ---
    let skirt_drop = SKIRT_DEPTH_SPACINGS * spacing;
    let last = side - 1;
    // Top (sz=0) and bottom (sz=last) full rows.
    (0..side).for_each(|sx| emit(&mut vertices, sx, 0, skirt_drop));
    (0..side).for_each(|sx| emit(&mut vertices, sx, last, skirt_drop));
    // Left (sx=0) and right (sx=last) columns, interior rows only (corners done).
    (1..last).for_each(|sz| emit(&mut vertices, 0, sz, skirt_drop));
    (1..last).for_each(|sz| emit(&mut vertices, last, sz, skirt_drop));

    // Local index of a surface grid vertex, and of a skirt twin by its border
    // position. The skirt block starts at CHUNK_VERTS; the order matches the
    // emission order above.
    let surf = |sx: usize, sz: usize| (sz * side + sx) as u32;
    let skirt_base = CHUNK_VERTS as u32;
    let skirt_top = |sx: usize| skirt_base + sx as u32; // sz=0 row
    let skirt_bottom = |sx: usize| skirt_base + (side + sx) as u32; // sz=last row
    let skirt_left = |sz: usize| skirt_base + (2 * side + (sz - 1)) as u32; // sx=0 interior
    let skirt_right = |sz: usize| skirt_base + (2 * side + (last - 1) + (sz - 1)) as u32; // sx=last

    let quads = side - 1; // 16
    let mut indices: Vec<u32> = Vec::with_capacity(quads * quads * 6 + 4 * quads * 6);

    // Surface quads (two triangles each), same winding as the original window.
    (0..quads).for_each(|gz| {
        (0..quads).for_each(|gx| {
            let i0 = surf(gx, gz);
            let i1 = surf(gx + 1, gz);
            let i2 = surf(gx, gz + 1);
            let i3 = surf(gx + 1, gz + 1);
            indices.extend_from_slice(&[i0, i2, i1, i1, i2, i3]);
        });
    });

    // Skirt quads: stitch each border edge (two adjacent border surface verts)
    // down to their dropped twins, forming the vertical curtain. Winding faces
    // outward; since terrain is opaque and the curtain only ever fills a gap,
    // exact facing is cosmetic.
    (0..quads).for_each(|gx| {
        // Top edge (sz = 0): surface (gx,0)-(gx+1,0) down to skirt twins.
        let s0 = surf(gx, 0);
        let s1 = surf(gx + 1, 0);
        let d0 = skirt_top(gx);
        let d1 = skirt_top(gx + 1);
        indices.extend_from_slice(&[s0, s1, d0, d0, s1, d1]);
        // Bottom edge (sz = last).
        let s0 = surf(gx, last);
        let s1 = surf(gx + 1, last);
        let d0 = skirt_bottom(gx);
        let d1 = skirt_bottom(gx + 1);
        indices.extend_from_slice(&[s0, d0, s1, s1, d0, d1]);
    });
    (0..quads).for_each(|gz| {
        // Left edge (sx = 0): twins use corners from the top/bottom rows.
        let s0 = surf(0, gz);
        let s1 = surf(0, gz + 1);
        let d0 = if gz == 0 {
            skirt_top(0)
        } else {
            skirt_left(gz)
        };
        let d1 = if gz + 1 == last {
            skirt_bottom(0)
        } else {
            skirt_left(gz + 1)
        };
        indices.extend_from_slice(&[s0, d0, s1, s1, d0, d1]);
        // Right edge (sx = last).
        let s0 = surf(last, gz);
        let s1 = surf(last, gz + 1);
        let d0 = if gz == 0 {
            skirt_top(last)
        } else {
            skirt_right(gz)
        };
        let d1 = if gz + 1 == last {
            skirt_bottom(last)
        } else {
            skirt_right(gz + 1)
        };
        indices.extend_from_slice(&[s0, s1, d0, d0, s1, d1]);
    });

    ChunkMesh { vertices, indices }
}

/// Assemble the combined terrain mesh the windowing backend uploads from the set
/// of cached per-chunk meshes.
///
/// `loaded` maps `ChunkKey` → that chunk's [`ChunkMesh`]. Keys are iterated in a
/// **deterministic sorted order** so the combined buffer is stable frame to frame
/// (only its contents change as chunks load/unload, not the order of unchanged
/// chunks). Each chunk contributes its `VERTS_PER_CHUNK` vertices verbatim and
/// its local indices offset by that chunk's base vertex (`k * VERTS_PER_CHUNK`).
fn assemble_chunks(loaded: &HashMap<ChunkKey, ChunkMesh>) -> (Vec<f32>, Vec<u32>) {
    let mut keys: Vec<ChunkKey> = loaded.keys().copied().collect();
    keys.sort_unstable();

    let mut vertices: Vec<f32> = Vec::with_capacity(keys.len() * VERTS_PER_CHUNK * VERT_FLOATS);
    let mut indices: Vec<u32> = Vec::new();

    for (k, key) in keys.iter().enumerate() {
        let chunk = &loaded[key];
        vertices.extend_from_slice(&chunk.vertices);
        let base = (k * VERTS_PER_CHUNK) as u32;
        indices.extend(chunk.indices.iter().map(|i| i + base));
    }

    (vertices, indices)
}

/// Total surface + skirt vertices across the loaded set (for telemetry).
fn total_vertices(loaded: &HashMap<ChunkKey, ChunkMesh>) -> usize {
    loaded.len() * VERTS_PER_CHUNK
}

/// The set of `(cx, cz, lod)` chunk keys that should be loaded for a player at
/// world position `(px, pz)` under `cfg` — the concentric LOD rings.
///
/// For each LOD `L` from 0 (finest) to `cfg.lod_levels-1` (coarsest) we take the
/// LOD-`L` chunks whose square `[origin, origin+size]` lies within the LOD-`L`
/// outer radius `cfg.outer_radius_m(L)` of the player (a filled square), and —
/// for `L > 0` — EXCLUDE any LOD-`L` chunk **fully contained** in the LOD-`(L-1)`
/// outer radius (the area finer LODs already cover). That yields a "ring with a
/// hole" per level; because each LOD grid is quadtree-aligned to the next and the
/// same hole boundary is used on both sides, the union tiles the ground with no
/// gap. A coarse chunk straddling the hole boundary is KEPT (it overlaps the
/// finer ring slightly, which — with skirts — guarantees no see-through hole at
/// the boundary rather than risking one).
fn lod_chunk_set(px: f32, pz: f32, cfg: &TerrainConfig) -> Vec<ChunkKey> {
    let mut out: Vec<ChunkKey> = Vec::new();
    (0..cfg.lod_levels).for_each(|lod| {
        let size = chunk_size_m(lod);
        let outer = cfg.outer_radius_m(lod);
        let inner = if lod == 0 {
            0.0
        } else {
            cfg.outer_radius_m(lod - 1)
        };

        // Range of LOD-`lod` chunk coords whose square can reach within `outer`
        // of the player. A chunk spans [c*size, (c+1)*size]; include it if that
        // span comes within `outer` of px (Chebyshev, per axis).
        let lo_x = ((px - outer) / size).floor() as i32;
        let hi_x = ((px + outer) / size).floor() as i32;
        let lo_z = ((pz - outer) / size).floor() as i32;
        let hi_z = ((pz + outer) / size).floor() as i32;

        (lo_z..=hi_z).for_each(|cz| {
            (lo_x..=hi_x).for_each(|cx| {
                let ox = cx as f32 * size;
                let oz = cz as f32 * size;
                // Nearest point of the chunk square to the player (0 per axis if
                // the player is inside the span) — "does it reach the outer ring?"
                let nearest_dx = (ox - px).max(px - (ox + size)).max(0.0);
                let nearest_dz = (oz - pz).max(pz - (oz + size)).max(0.0);
                let reaches_outer = nearest_dx <= outer && nearest_dz <= outer;
                if !reaches_outer {
                    return;
                }
                // Farthest corner of the chunk square from the player — "is the
                // whole chunk already inside the finer LOD's hole?"
                let far_dx = (px - ox).abs().max(ox + size - px);
                let far_dz = (pz - oz).abs().max(oz + size - pz);
                let fully_in_hole = lod > 0 && far_dx <= inner && far_dz <= inner;
                if fully_in_hole {
                    return;
                }
                out.push((cx, cz, lod));
            });
        });
    });
    out
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

    let seed = growth.seed.value;

    // Read the distance-LOD config JS pushed via set_terrain_config (or default).
    let cfg = TERRAIN_CONFIG.with(|c| *c.borrow());
    log(&format!(
        "descend(): LOD config levels={} lod0_radius={} bias={:.0}m draw≈{:.0}m",
        cfg.lod_levels,
        cfg.lod0_radius_chunks,
        cfg.detail_bias_m,
        cfg.draw_distance_m()
    ));

    // Initial load: pre-generate the FULL desired LOD ring set around the spawn
    // (player at world origin) with NO per-frame cap — the one-time "load". The
    // combined buffer assembled from these is the geometry handed to
    // `run_web_streaming`; the closure then streams incrementally from here.
    let mut loaded: HashMap<ChunkKey, ChunkMesh> = HashMap::new();
    for (cx, cz, lod) in lod_chunk_set(0.0, 0.0, &cfg) {
        loaded.insert(
            (cx, cz, lod),
            gen_chunk(&growth, &localmap, seed, &cfg, cx, cz, lod, anchor_h),
        );
    }
    let (init_vertices, init_indices) = assemble_chunks(&loaded);
    log(&format!(
        "[lod] chunks={} verts={} draw={:.0}m (initial, anchor_h={:.1}m)",
        loaded.len(),
        init_vertices.len() / VERT_FLOATS,
        cfg.draw_distance_m(),
        anchor_h
    ));

    // The player starts at the anchor (spawn origin), eye at recentred ground 0.
    let start_ground = 0.0; // chunks are recentred so the anchor surface is y = 0.
    let eye_y = start_ground + EYE_HEIGHT_M;
    let mut running = build_viewer_app(eye_y);

    let player = Rc::new(RefCell::new(Player {
        x: 0.0,
        z: 0.0,
        yaw: 0.0,
        pitch: 0.0,
        engine_y: eye_y,
    }));

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
        init_vertices,
        init_indices,
        Texture::BiomeAtlas.rgba(), // the terrain's albedo: the biome atlas
        1,                          // one renderable (the terrain) -> one instance
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

            // --- 5. Stream the terrain incrementally as LOD ring chunks. The
            // player lives in continuous world space; each frame we recompute the
            // desired concentric LOD set around them, generate only the chunks
            // that newly enter it (bounded to MAX_GEN_PER_FRAME, nearest first, so
            // there is NO all-at-once spike — a coarse far chunk costs the same as
            // a near one) and drop chunks that leave the set (with a one-chunk
            // hysteresis margin so ring boundaries don't thrash). Heights/normals/
            // colours are pure functions of world position recentred by the SAME
            // fixed anchor, so chunks of one LOD are seamless with each other and
            // stable across frames; LOD boundaries are bridged by skirts. We only
            // reassemble + re-upload on frames where the loaded set changed. ---
            let desired: Vec<ChunkKey> = lod_chunk_set(player_x, player_z, &cfg);
            let desired_set: std::collections::HashSet<ChunkKey> =
                desired.iter().copied().collect();

            // (a) Generate up to MAX_GEN_PER_FRAME missing desired chunks, nearest
            // (by chunk centre) to the player first.
            let mut missing: Vec<ChunkKey> = desired
                .iter()
                .copied()
                .filter(|key| !loaded.contains_key(key))
                .collect();
            let center_d2 = |&(cx, cz, lod): &ChunkKey| -> f32 {
                let size = chunk_size_m(lod);
                let mx = (cx as f32 + 0.5) * size - player_x;
                let mz = (cz as f32 + 0.5) * size - player_z;
                mx * mx + mz * mz
            };
            missing.sort_by(|a, b| {
                center_d2(a)
                    .partial_cmp(&center_d2(b))
                    .unwrap_or(std::cmp::Ordering::Equal)
            });
            let added = missing.len().min(MAX_GEN_PER_FRAME);
            for &(cx, cz, lod) in missing.iter().take(MAX_GEN_PER_FRAME) {
                loaded.insert(
                    (cx, cz, lod),
                    gen_chunk(&growth, &localmap, seed, &cfg, cx, cz, lod, anchor_h),
                );
            }

            // (b) Drop loaded chunks no longer desired, but keep ones still within
            // a one-chunk hysteresis margin of their LOD's outer radius so chunks
            // straddling a ring boundary aren't loaded/unloaded every frame.
            let before = loaded.len();
            loaded.retain(|&(cx, cz, lod), _| {
                if desired_set.contains(&(cx, cz, lod)) {
                    return true;
                }
                let size = chunk_size_m(lod);
                let keep = cfg.outer_radius_m(lod) + size; // +1 chunk slack
                let ox = cx as f32 * size;
                let oz = cz as f32 * size;
                let nearest_dx = (ox - player_x).max(player_x - (ox + size)).max(0.0);
                let nearest_dz = (oz - player_z).max(player_z - (oz + size)).max(0.0);
                nearest_dx <= keep && nearest_dz <= keep
            });
            let removed = before - loaded.len();

            // (c) Reassemble + re-upload only when the loaded set changed, and log
            // telemetry (chunk count, vertex count, draw distance) on those frames
            // so the LOD effect is measurable from the browser console.
            let changed = added > 0 || removed > 0;
            let new_geometry = changed.then(|| {
                log(&format!(
                    "[lod] chunks={} verts={} draw={:.0}m (+{added} -{removed})",
                    loaded.len(),
                    total_vertices(&loaded),
                    cfg.draw_distance_m(),
                ));
                assemble_chunks(&loaded)
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
