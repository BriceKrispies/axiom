//! # The Procedural Mesh Crucible
//!
//! A browser/WASM composition-leaf app whose single job is to prove the
//! `axiom-mesh` / `axiom-mesh-ops` layers in the only way that counts: by
//! building a coherent scene in which **every visible triangle came out of a
//! mesh operator**. There is no imported geometry in this crate, no hand-typed
//! vertex array, and no engine primitive standing in for a generated one.
//!
//! ## What is on screen
//!
//! Eight concentric, alternately counter-rotating rings of walking
//! **dachshunds** — 104 of them, packed nose to tail — on generated ground.
//!
//! | Object | Operator chain |
//! |---|---|
//! | terrain | `heightfield_mesh` over an analytic sine sum, with a skirt |
//! | dachshund ×104 | `loft` torso halves + tapered `sweep` neck/muzzle/ears/legs/tail + `icosphere` skull + `uv_sphere` nose + `rounded_box` paws, cut at the joints into a 23-bone rig |
//!
//! The rings run from radius 26 (the tightest curve the gait is tuned for) out to
//! radius 80.25 (the widest circle that still leaves half a dog's length of clear
//! ground before the terrain's rim), 7.75 units apart — a pitch set by the dog's
//! **width**, not its length. Each successive ring reverses direction, so every
//! ring turns against both of its neighbours.
//!
//! The dog is 2.40 units long and 0.79 tall in its own space — **three times as
//! long as it is high**, on legs a sixth of its length. That single ratio is
//! what sizes the rest of the app: a longer body bulges further off a curve, so
//! the radial pitch and the innermost radius are re-derived from it, and shorter
//! legs have less absolute reach, so the stride, the crouch and the terrain the
//! feet may follow are re-derived from it too. `tests/creatures.rs` holds the
//! proportion band and `tests/locomotion.rs` holds the reach.
//!
//! ## One dog's geometry, 104 dogs on screen — and 415 draw calls
//!
//! Every dog in the field is the **same 23 registered meshes**, wearing one of
//! **18 shared coats**. `crucible_scene` returns the distinct mesh set
//! (`objects`) and the crowd (`dogs`) as two separate things precisely so they
//! cannot be conflated: adding a dog costs a transform and a palette index, never
//! a vertex and never a material.
//!
//! The material half of that is not decoration. The live backend batches draws on
//! the `(mesh_id, material_id)` pair and a draw's colour reaches the GPU only
//! through its material, so one material per dog would be `23 × 104 = 2392`
//! single-instance batches. A bounded palette caps the draw-call count at
//! `23 × 18 + 1 = 415` for **any** crowd size — the field as laid out wears every
//! one of the 18 coats, so it reaches exactly that — and `tests/rings.rs` holds
//! both that bound and the distinct-mesh count directly.
//!
//! ## The walkers
//!
//! Each dog is spawned one scene object **per bone**, and every frame the app
//! re-authors those bones' instance transforms from a pose that is a pure
//! function of the engine tick: a closed `Curve::catmull_rom` ring, a
//! distance-driven gait whose planted paws do not move while the body travels
//! over them, and two-bone analytic inverse kinematics from each hip to each paw.
//! A dog's place in its chain is a fixed arc-length offset, which spaces the ring
//! evenly *and* puts every dog at a different point in the trot — the legs run as
//! a wave around the ring instead of stamping in lockstep.
//!
//! Rigid bones rather than GPU skinning is a deliberate constraint, not a
//! shortcut: the WebGL2 fallback this app must survive on has no vertex-stage
//! storage buffers and draws no skinned geometry at all, whereas a re-authored
//! instance transform is an ordinary instanced draw. See `creature_rig.rs`.
//!
//! ## The topology proof
//!
//! [`CrucibleVariant`] changes nothing but tessellation counts, and
//! [`crucible_meshes`] is a pure function of it. Building the scene at `Base`,
//! `Dense` and `Coarse` therefore produces the same objects, in the same order,
//! with the same materials — and different vertex and index counts. That is the
//! claim `tests/crucible_scene.rs` makes concrete, object by object, with the
//! numbers printed.
//!
//! ## Layering
//!
//! The app depends on the `kernel`, `math`, `mesh` and `mesh-ops` layers and on
//! the `engine` and `windowing` modules — and on nothing else. It never names
//! `axiom-proc-mesh`, `axiom-resources`, `axiom-render` or any backend: geometry
//! comes from the mesh layers, and everything downstream of that is the engine
//! umbrella's business. The ring layout, the rainbow and the crowd are **app
//! composition** and live here; no ring, colour or crowd vocabulary was added to
//! a mesh layer, and none should be.

mod debug_view;
mod install;
mod object;
mod orbit;
mod quantities;
mod rainbow;
mod rings;
mod scene;
mod terrain;
mod variant;

/// The browser edge: the `#[wasm_bindgen]` entry, the query-string read, the
/// device-sized surface, and the live present loop. Compiled only for `wasm32`.
#[cfg(target_arch = "wasm32")]
pub mod live;

/// The DOM half of the orbit camera: pointer/wheel gestures measured and handed
/// to [`orbit::OrbitState`]. Compiled only for `wasm32`; the camera policy it
/// drives is browser-free and lives in `src/orbit.rs`.
#[cfg(target_arch = "wasm32")]
mod pointer_input;

pub use debug_view::{chart_rgba, DebugView, CHART_SIZE};
pub use install::{install_crucible, InstalledCrucible};
pub use object::CrucibleObject;
pub use orbit::OrbitState;
pub use rainbow::{hsv_to_rgb, hue_to_rgb, RING_SATURATION, RING_VALUE};
pub use rings::{
    body_bulge, dog_total, outer_clearance, palette, palette_color, ring_dogs, Ring, RingDog,
    Winding, DOG_BODY_LENGTH, DOG_BODY_WIDTH, DOG_GAP, DOG_LENGTH, DOG_SCALE, DOG_SPACING,
    DOG_WIDTH, PALETTE_SIZE, RINGS, RING_COMB, RING_COUNT, RING_MAX_RADIUS, RING_MIN_RADIUS,
    RING_SPACING,
};
pub use scene::{crucible_meshes, crucible_scene, CrucibleScene};
pub use terrain::{ground_y, TERRAIN_HALF_EXTENT};
pub use variant::{CrucibleVariant, DetailParams};

use axiom::prelude::RunningApp;

/// The presentation surface the page lays out and the live backend binds to.
pub const CANVAS_ID: &str = "axiom-crucible-canvas";

/// The authored surface size. The page scales the canvas with CSS; this is the
/// framebuffer the projection's aspect is resolved against.
pub const WIDTH: u32 = 1280;
pub const HEIGHT: u32 = 720;

/// Build the crucible as a headless [`RunningApp`] — the native path the
/// integration tests and any capture harness use. Identical to what the browser
/// entry presents, minus the surface.
pub fn crucible_core(variant: CrucibleVariant, view: DebugView) -> RunningApp {
    crucible_animated(variant, view).0
}

/// Build the crucible headlessly *and* keep the handle that animates it — the
/// native path a locomotion test drives.
pub fn crucible_animated(
    variant: CrucibleVariant,
    view: DebugView,
) -> (RunningApp, InstalledCrucible) {
    use axiom::prelude::{App, Color, DefaultPlugins, Ratio, Window};
    let mut running = App::new()
        .window(
            Window::new(WIDTH, HEIGHT)
                .with_surface_id(CANVAS_ID)
                .with_clear_color(Color::linear_rgb(
                    Ratio::finite_or_zero(0.05),
                    Ratio::finite_or_zero(0.07),
                    Ratio::finite_or_zero(0.11),
                )),
        )
        .add_plugins(DefaultPlugins)
        .setup(|_world, _meshes, _materials| {})
        .build();
    let installed = install_crucible(&mut running, variant, view, None);
    (running, installed)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_native_core_renders_every_spawned_instance() {
        let scene = crucible_scene(CrucibleVariant::Base).expect("the base scene is valid geometry");
        let statics = scene.dog_first;
        let bones = scene.objects.len() - statics;
        let instances = statics + bones * scene.dogs.len();
        let mut app = crucible_core(CrucibleVariant::Base, DebugView::Shaded);
        let outcome = app.tick(0);
        // One draw per spawned instance — the terrain plus every bone of every
        // dog — off `scene.objects.len()` registered meshes and no more.
        assert_eq!(outcome.draws().len(), instances);
        // ...and those instances collapse into at most `bones × PALETTE_SIZE + 1`
        // batches, because the coats are shared. This is the number that decides
        // the frame rate, and it does not grow with the crowd.
        let batches = outcome.mesh_batches().len();
        assert!(
            batches <= bones * PALETTE_SIZE + statics,
            "{batches} batches for a {PALETTE_SIZE}-entry palette"
        );
        assert!(batches * 4 < instances, "{batches} batches, {instances} instances");
        // Three lights: the sun and two fills.
        assert_eq!(outcome.lights().len(), 3);
        assert_eq!(outcome.clear_color(), [0.05, 0.07, 0.11, 1.0]);
    }

    #[test]
    fn the_chart_texture_is_a_well_formed_rgba_image() {
        let pixels = chart_rgba();
        assert_eq!(pixels.len(), (CHART_SIZE * CHART_SIZE * 4) as usize);
        // Every texel is opaque, and the chart is not a flat colour.
        assert!(pixels.chunks_exact(4).all(|texel| texel[3] == 255));
        assert_ne!(pixels[0..3], pixels[pixels.len() - 4..pixels.len() - 1]);
    }
}

/// The articulated dog. A dog is a **semantic** shape, so it is a composition of
/// the generic operators authored here in the app — `axiom-mesh-ops` has no
/// anatomy in it and must never acquire any.
mod creature_dog;
mod creature_rig;

pub use creature_dog::{dog, dog_limbs, dog_parts};
pub use creature_rig::{CreatureRig, LimbChain, RigPart};

/// The locomotion the dogs walk: the two closed rings, the distance-driven gait,
/// the two-bone inverse kinematics that plant their paws, and the per-tick pose
/// that composes the three.
mod creature_pose;
mod leg_ik;
mod locomotion;

pub use creature_pose::{CreaturePose, DOG_GAIT};
pub use leg_ik::{ease, solve_two_bone, stride_phase, swing_lift, StridePhase, TwoBone};
pub use locomotion::{dog_travel, CrucibleAnimation, LoopPath, PathPoint, TRAVEL_PER_TICK};
