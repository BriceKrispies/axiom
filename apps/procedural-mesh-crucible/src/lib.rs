//! # The Procedural Mesh Crucible
//!
//! A browser/WASM composition-leaf app whose single job is to prove the
//! `axiom-mesh` / `axiom-mesh-ops` layers in the only way that counts: by
//! building a coherent scene in which **every visible triangle came out of a
//! mesh operator**. There is no imported geometry in this crate, no hand-typed
//! vertex array, and no engine primitive standing in for a generated one.
//!
//! ## What is in the scene, and what made it
//!
//! | Object | Operator chain |
//! |---|---|
//! | terrain | `heightfield_mesh` over an analytic sine sum, with a skirt |
//! | road | `sweep` of a slab profile along a `Curve::catmull_rom`, banked with `SweepOptions::twist` |
//! | tunnel | `sweep` of a closed arch profile along a path derived from the road, `CapPolicy::None` |
//! | vehicle | `loft` through five sections + `revolve` wheels + `box_mesh`/`cube` details, `combine`d |
//! | trees ×4 | tapered `sweep` trunk (`start_scale` > `end_scale`) + `icosphere`/`capsule` crown |
//! | building | `extrude` of a **concave L** footprint, stacked into floors |
//! | sculpture | `implicit_surface_mesh` over a smooth-min blend of five sphere SDFs |
//! | reference row | every primitive the library ships, once each, in a line |
//! | detail ladder | one shape at four densities, plus `subdivide_loop` vs `simplify_quadric` |
//! | dog + human | the same operators cut into a **bone rig** and run around the scene |
//!
//! ## The two runners
//!
//! The dog and the human are not statues. Each is spawned one scene object **per
//! bone**, and every frame the app re-authors those bones' instance transforms
//! from a pose that is a pure function of the engine tick: a closed
//! `Curve::catmull_rom` loop around the perimeter, a distance-driven gait whose
//! planted feet do not move while the body travels over them, and two-bone
//! analytic inverse kinematics from each hip to each foot. The dog trots a
//! diagonal pattern; the human runs a fixed arc-length behind it on the same
//! track, arms counter-swinging its legs.
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
//! umbrella's business.

mod building;
mod curves;
mod debug_view;
mod detail_ladder;
mod flora;
mod install;
mod object;
mod orbit;
mod primitive_row;
mod quantities;
mod road;
mod scene;
mod sculpture;
mod terrain;
mod variant;
mod vehicle;

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
pub use scene::{crucible_meshes, crucible_scene, CrucibleScene};
pub use curves::road_curve;
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
    install_crucible(&mut running, variant, view, None);
    running
}

/// Build the crucible headlessly *and* keep the handle that animates it — the
/// native path a locomotion test drives.
pub fn crucible_animated(variant: CrucibleVariant, view: DebugView) -> (RunningApp, InstalledCrucible) {
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
    fn the_native_core_renders_every_generated_object() {
        let objects =
            crucible_meshes(CrucibleVariant::Base).expect("the base scene is valid geometry");
        let mut app = crucible_core(CrucibleVariant::Base, DebugView::Shaded);
        let outcome = app.tick(0);
        // One draw per generated object, each with its own mesh and material —
        // `renderable_count` is the *authoring* count and stays zero here,
        // because the crucible installs everything through the runtime
        // `spawn`/`add_mesh_data` path rather than through `setup`.
        assert_eq!(outcome.draws().len(), objects.len());
        assert_eq!(outcome.mesh_batches().len(), objects.len());
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

/// The two articulated creatures. A dog and a human are **semantic** shapes, so
/// they are compositions of the generic operators authored here in the app —
/// `axiom-mesh-ops` has no anatomy in it and must never acquire any.
mod creature_dog;
mod creature_human;
mod creature_rig;

pub use creature_dog::{dog, dog_parts};
pub use creature_human::{human, human_parts};
pub use creature_rig::{CreatureRig, LimbChain, RigPart};

/// The locomotion the two creatures run: the closed path around the scene, the
/// distance-driven gait, the two-bone inverse kinematics that plant their feet,
/// and the per-tick pose that composes the three.
mod creature_pose;
mod leg_ik;
mod locomotion;

pub use creature_pose::{CreaturePose, DOG_GAIT, HUMAN_GAIT};
pub use leg_ik::{ease, solve_two_bone, stride_phase, swing_lift, StridePhase, TwoBone};
pub use creature_dog::dog_limbs;
pub use creature_human::human_limbs;
pub use locomotion::{
    dog_travel, human_travel, CrucibleAnimation, LoopPath, PathPoint, HUMAN_LAG, LOOP_RADIUS,
    TRAVEL_PER_TICK,
};
