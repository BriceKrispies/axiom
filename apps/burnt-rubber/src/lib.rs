//! **Burnt Rubber** — an original third-person arcade racing framework and
//! demonstration game.
//!
//! Nine kilometres of road, **compiled once from an authored course
//! specification**, with a pacing curve rather than uniform noise: an opening
//! straight to learn the throttle on, long sweepers, rolling crests, a set of
//! esses, a lit tunnel, a wide traffic-choked straight, a canyon squeeze, a
//! closing sweep and a finish arch. The road, its traffic, its authored
//! encounters and its near-miss opportunities are all produced by [`course`] —
//! from Rust for the shipping course, or from a `.brc` source for a
//! hand-authored one — and validated before anybody drives them. See
//! `COURSES.md`. The car is an authored arcade model — instant acceleration,
//! speed-sensitive steering, handbrake oversteer, a forgiving drift window — and
//! the reward loop is one number: fill the boost meter by threading traffic,
//! holding a drift, or simply staying flat out, and spend it on more of all
//! three.
//!
//! # Architecture
//!
//! This is a **composition-leaf app**. Every racing concept lives here and
//! nowhere else: the car model, the chase camera, boost, drifting, the course
//! generator, the road mesh, the chunk lifecycle, the roadside scenery, the
//! traffic, near misses, the racing HUD, the racing collision responses and the
//! racing telemetry. None of it is in the kernel, none of it is a new layer, and
//! there is no "generic vehicle physics module" — a vehicle model this authored
//! is a game design decision, not an engine capability, and the moment it were
//! shared it would stop being tunable for *this* game.
//!
//! What it *reuses* from the engine is deliberately broad:
//!
//! | Engine capability | Used for |
//! |---|---|
//! | `axiom` (the umbrella) | the scene, meshes, materials, lights, camera, the render tick |
//! | `axiom_kernel::DeterministicRng` | the seeded course, scenery and traffic streams |
//! | `axiom_math` | every vector, quaternion and transform |
//! | `axiom_frame::FrameAccumulator` | banking a variable browser frame into whole 60 Hz steps |
//! | `axiom_input::InputState` | the deterministic action-binding table |
//! | `axiom_visibility::VisibilityApi` | frustum culling and distance-band LOD for the scenery pool |
//! | `axiom_audio::AudioApi` | the engine, wind, tyre, boost and impact cues |
//! | `axiom_windowing` (wasm) | the live presentation loop |
//!
//! # The deterministic boundary
//!
//! Everything under [`sim`] plus [`track`] and [`camera`] is the deterministic
//! half: it advances only on fixed 60 Hz steps, reads only a [`DriveCommand`],
//! and never touches a clock. Everything under [`render`], plus [`hud`] and the
//! `wasm32` [`web`] arm, is presentation: it *reads* simulation state and
//! interpolates it, and writes nothing back. [`BurntRubber`] is the seam — it
//! owns both halves and is the only place they meet.

pub mod camera;
pub mod command;
/// The course authoring, compilation, validation and runtime system.
pub mod course;
pub mod controls;
pub mod diagnostics;
pub mod draw;
pub mod hud;
pub mod profile;
pub mod sim;
pub mod start_screen;
pub mod telemetry;
pub mod touch;
pub mod track;
pub mod tuning;

pub mod audio_cues;
pub mod debug_view;
pub mod render;

pub mod app;
pub mod capture;
// A deterministic, steppable control surface over the live browser session —
// how a motion-only defect (crawl, shimmer) is diagnosed, since a still cannot
// show one. Inert unless a probe command is issued.
pub mod probe;
pub mod script;

/// Playing the race through the `axiom-agent` substrate.
pub mod agent;
/// The agent, running live as a translucent ghost you race against.
pub mod ghost;

#[cfg(target_arch = "wasm32")]
pub mod web;
#[cfg(target_arch = "wasm32")]
pub use web::burnt_rubber_start;

pub use app::{build_burnt_rubber, BurntRubber};
pub use camera::{CameraPose, ChaseCamera};
pub use command::DriveCommand;
pub use profile::PlayProfile;
pub use course::runtime::CoursePlan;
pub use course::specification::CourseSpec;
pub use course::{compile as compile_course, CourseError, ValidationReport};
pub use sim::{RaceEvent, RacePhase, RaceSim};
pub use start_screen::{StartCommand, StartOutcome, StartScreen};
pub use track::{SectionKind, Track, TrackSample, Zone};
pub use tuning::Tuning;

/// The canvas id the browser build binds its surface to.
pub const CANVAS_ID: &str = "axiom-burnt-rubber-canvas";

/// The seed the shipping course is generated from.
///
/// One fixed seed, not a clock reading: the demo course is a *designed* course
/// that happens to have been produced procedurally, and everyone who plays it
/// drives the same road. Changing this number changes the game.
pub const DEFAULT_SEED: u64 = 0x0B17_4E7A_5C09_1D33;

/// The **fallback** frame size — a nominal 16:9 pair, used where no real
/// surface has been measured.
///
/// It is explicitly *not* what the live app renders at. The browser arm asks
/// `WindowingApi::configure_surface_from_canvas` for the canvas's actual box in
/// device pixels and builds the app from that, because a canvas laid out by CSS
/// (`100vw x 100vh` on a phone) has neither this size nor this shape, and a
/// camera resolved against a number the display does not honour renders a world
/// squeezed by the ratio between the two. What is left for these constants is
/// the native tests, and the window-size fallback for a page with no `window`.
pub const WIDTH: u32 = 1280;
/// See [`WIDTH`].
pub const HEIGHT: u32 = 720;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_shipping_constants_are_stable() {
        assert_eq!(CANVAS_ID, "axiom-burnt-rubber-canvas");
        assert_eq!(DEFAULT_SEED, 0x0B17_4E7A_5C09_1D33);
        assert_eq!((WIDTH, HEIGHT), (1280, 720));
    }

    /// The one course everybody drives has to be a good one, so its headline
    /// properties are pinned here rather than left to the generator's mood.
    #[test]
    fn the_shipping_course_is_the_designed_course() {
        let plan = course::procedural::shipping_plan(DEFAULT_SEED)
            .expect("the shipping course compiles");
        assert!(
            (8_000.0..=10_500.0).contains(&plan.length()),
            "the demo course is 8-10 km: {} m",
            plan.length()
        );
        assert!(plan.track().samples().len() > 4_000);
        assert_eq!(plan.seed(), DEFAULT_SEED);
        assert!(
            !plan.report().has_errors(),
            "and it validates:
{}",
            plan.report().dump()
        );
    }
}
