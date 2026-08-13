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
//! Two counter-rotating rings of walking dogs, on generated ground.
//!
//! | Object | Operator chain |
//! |---|---|
//! | terrain | `heightfield_mesh` over an analytic sine sum, with a skirt |
//! | dog ×19 | `loft` torso halves + tapered `sweep` neck/muzzle/ears/legs/tail + `icosphere` skull + `uv_sphere` nose + `rounded_box` paws, cut at the joints into a 23-bone rig |
//!
//! The outer ring (radius 46) holds 12 dogs walking anticlockwise seen from
//! above; the inner ring (radius 26) holds 7 walking the other way. Each ring is
//! painted across the full hue circle, half a turn apart, so no two neighbours
//! anywhere share a colour.
//!
//! ## One dog's geometry, nineteen dogs on screen
//!
//! Every dog in both rings is the **same 23 registered meshes**. `crucible_scene`
//! returns the distinct mesh set (`objects`) and the crowd (`dogs`) as two
//! separate things precisely so they cannot be conflated: adding a dog costs a
//! transform and a colour, never a vertex. `install.rs` registers the mesh set
//! once and spawns `dogs.len() × 23` instances of it — `tests/rings.rs` asserts
//! that distinct-mesh count directly, because "the geometry is shared" is a
//! claim, and a claim in this app is something a test holds.
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
    ring_dogs, Ring, RingDog, Winding, DOG_BODY_LENGTH, DOG_GAP, DOG_LENGTH, DOG_SCALE,
    DOG_SPACING, INNER, OUTER, RINGS,
};
pub use scene::{crucible_meshes, crucible_scene, CrucibleScene};
pub use terrain::ground_y;
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
        assert_eq!(outcome.mesh_batches().len(), instances);
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
