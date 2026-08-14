//! # Dog
//!
//! A browser/WASM composition-leaf app: concentric rings of walking
//! **dachshunds** on generated ground, where **every visible triangle came out of
//! a mesh operator**. There is no imported geometry in this crate, no hand-typed
//! vertex array, and no engine primitive standing in for a generated one.
//!
//! ## What is on screen
//!
//! | Object | Operator chain |
//! |---|---|
//! | terrain | `heightfield_mesh` over an analytic sine sum, with a skirt |
//! | dachshund | `loft` torso halves + tapered `sweep` neck/muzzle/ears/legs/tail + `icosphere` skull + `uv_sphere` nose + `rounded_box` paws, cut at the joints into a 23-bone rig |
//!
//! At its defaults the field is eight rings from radius 26 out to 80.25, 7.75
//! apart, holding 104 dogs — a layout that is *derived* rather than typed, from
//! the dog's own length, width and the room it needs on an arc.
//!
//! The dog is 2.40 units long and 0.79 tall in its own space — **three times as
//! long as it is high**, on legs a sixth of its length. That single ratio is
//! what sizes the rest of the app: a longer body bulges further off a curve, so
//! the radial pitch and the innermost radius are re-derived from it, and shorter
//! legs have less absolute reach, so the stride, the crouch and the terrain the
//! feet may follow are re-derived from it too. `tests/creatures.rs` holds the
//! proportion band and `tests/locomotion.rs` holds the reach.
//!
//! ## The panel: fifteen dials, one value
//!
//! The page is a canvas and a slider panel. Every slider is a [`Dial`], every
//! dial is a field of the one [`SceneConfig`] value the whole scene is a pure
//! function of, and the panel itself is *generated* from [`Dial::ALL`] — so a
//! slider cannot exist without a dial behind it and a dial cannot exist without
//! a slider in front of it.
//!
//! Fourteen of the fifteen re-pose the running scene instantly, because a frame
//! is allowed to change an instance transform and a visibility flag and nothing
//! else. The fifteenth (`detail`) re-tessellates geometry, which the live backend
//! uploads once at bind, so it round-trips through the query string and reloads.
//! `NOTES.md` records why that boundary is where it is.
//!
//! Every clamp that keeps the scene legal — a stride the leg cannot pay for, a
//! ring pitch tighter than the dogs are wide, a crowd larger than the instance
//! pool — is *derived* rather than trusted to the slider's own range. See
//! [`crate::config`] and [`crate::rings`].
//!
//! ## One dog's geometry, a field of dogs — and 415 draw calls
//!
//! Every dog in the field is the **same 23 registered meshes**, wearing one of
//! **18 shared coats**. [`build_scene`] returns the distinct mesh set (`objects`)
//! and the crowd (`dogs`) as two separate things precisely so they cannot be
//! conflated: adding a dog costs a transform and a palette index, never a vertex
//! and never a material.
//!
//! The material half of that is not decoration. The live backend batches draws on
//! the `(mesh_id, material_id)` pair and a draw's colour reaches the GPU only
//! through its material, so one material per dog would be `23 × dogs`
//! single-instance batches. A bounded palette caps the draw-call count at
//! `23 × 18 + 1 = 415` for **any** crowd size, and `tests/rings.rs` holds that
//! bound and the distinct-mesh count directly.
//!
//! ## The walkers
//!
//! Each dog is drawn as one scene node **per bone**, and every frame the app
//! re-authors those bones' instance transforms from a pose that is a pure
//! function of the engine tick *and the configuration*: a closed
//! `Curve::catmull_rom` ring, a distance-driven gait whose planted paws do not
//! move while the body travels over them, and two-bone analytic inverse
//! kinematics from each hip to each paw. A dog's place in its chain is a fixed
//! arc-length offset, which spaces the ring evenly *and* puts every dog at a
//! different point in the trot — the legs run as a wave around the ring instead
//! of stamping in lockstep.
//!
//! Rigid bones rather than GPU skinning is a deliberate constraint, not a
//! shortcut: the WebGL2 fallback this app must survive on has no vertex-stage
//! storage buffers and draws no skinned geometry at all, whereas a re-authored
//! instance transform is an ordinary instanced draw. See `creature_rig.rs`.
//!
//! ## The topology proof
//!
//! [`SceneVariant`] changes nothing but tessellation counts, and
//! [`scene_meshes`] is a pure function of it. Building the scene at `Base`,
//! `Dense` and `Coarse` therefore produces the same objects, in the same order,
//! with the same materials — and different vertex and index counts. That is the
//! claim `tests/scene.rs` makes concrete, object by object, with the numbers
//! printed.
//!
//! ## Layering
//!
//! The app depends on the `kernel`, `math`, `mesh` and `mesh-ops` layers and on
//! the `engine` and `windowing` modules — and on nothing else. It never names
//! `axiom-proc-mesh`, `axiom-resources`, `axiom-render` or any backend: geometry
//! comes from the mesh layers, and everything downstream of that is the engine
//! umbrella's business. The ring layout, the rainbow, the crowd and the dial
//! panel are **app composition** and live here; no ring, colour, crowd or slider
//! vocabulary was added to a mesh layer, and none should be.

mod config;
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

/// The DOM half of the dial panel: the sliders are *built* from [`Dial::ALL`] and
/// write into the shared [`SceneConfig`] the frame closure reads. Compiled only
/// for `wasm32`; every dial's meaning, range and clamp is browser-free and lives
/// in `src/config.rs`.
#[cfg(target_arch = "wasm32")]
mod slider_input;

pub use config::{Dial, DialSpec, SceneConfig, DIAL_COUNT};
pub use debug_view::{chart_rgba, DebugView, CHART_SIZE};
pub use install::{install_scene, InstalledScene};
pub use object::SceneObject;
pub use orbit::OrbitState;
pub use rainbow::{hsv_to_rgb, hue_to_rgb, RING_SATURATION, RING_VALUE};
pub use rings::{
    body_bulge, dog_total, inner_radius, min_ring_spacing, outer_clearance, palette, palette_color,
    ring_count, ring_radius, ring_spacing, ring_dogs, rings, Ring, RingDog, Winding,
    DOG_BODY_LENGTH, DOG_BODY_WIDTH, MAX_DOGS, MAX_RINGS, PALETTE_SIZE, RING_AIR,
};
pub use scene::{build_scene, scene_meshes, Scene};
pub use terrain::{ground_y, TERRAIN_HALF_EXTENT};
pub use variant::{SceneVariant, DetailParams};

use axiom::prelude::RunningApp;

/// The presentation surface the page lays out and the live backend binds to.
pub const CANVAS_ID: &str = "axiom-dog-canvas";

/// The authored surface size. The page scales the canvas with CSS; this is the
/// framebuffer the projection's aspect is resolved against.
pub const WIDTH: u32 = 1280;
pub const HEIGHT: u32 = 720;

/// Build the field as a headless [`RunningApp`] — the native path the
/// integration tests and any capture harness use. Identical to what the browser
/// entry presents, minus the surface.
pub fn headless_app(variant: SceneVariant, view: DebugView, config: &SceneConfig) -> RunningApp {
    headless_animated(variant, view, config).0
}

/// Build the field headlessly *and* keep the handle that animates it — the
/// native path a locomotion test drives.
pub fn headless_animated(
    variant: SceneVariant,
    view: DebugView,
    config: &SceneConfig,
) -> (RunningApp, InstalledScene) {
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
    let installed = install_scene(&mut running, variant, view, None, config);
    (running, installed)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_native_core_draws_the_crowd_the_layout_asks_for_and_no_more() {
        let config = SceneConfig::defaults();
        let scene = build_scene(SceneVariant::Base, &config).expect("the base scene is valid");
        let statics = scene.dog_first;
        let bones = scene.objects.len() - statics;
        let instances = statics + bones * scene.dogs.len();
        let mut app = headless_app(SceneVariant::Base, DebugView::Shaded, &config);
        let outcome = app.tick(0);
        // One draw per *visible* instance — the terrain plus every bone of every
        // dog the layout placed. The retired pool slots cost nothing: an
        // invisible renderable is dropped at submission.
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

pub use creature_dog::{
    dog, dog_limbs, dog_parts, front_hip_drop, front_leg_reach, wheelbase_local,
};
pub use creature_rig::{CreatureRig, LimbChain, RigPart};

/// The locomotion the dogs walk: the closed rings, the distance-driven gait, the
/// two-bone inverse kinematics that plant their paws, and the per-tick pose that
/// composes the three.
mod creature_pose;
mod leg_ik;
mod locomotion;

pub use creature_pose::{Gait, DOG_GAIT};
pub use leg_ik::{ease, solve_two_bone, stride_phase, swing_lift, StridePhase, TwoBone};
pub use locomotion::{
    dog_travel, Animation, LoopPath, PathPoint, DEFAULT_TRAVEL_PER_TICK,
};
