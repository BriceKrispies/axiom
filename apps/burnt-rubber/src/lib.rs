//! **Burnt Rubber** — an original third-person arcade racing framework and
//! demonstration game.
//!
//! Nine kilometres of procedurally generated road, generated once from a single
//! seed, with a pacing curve rather than uniform noise: an opening straight to
//! learn the throttle on, long sweepers, rolling crests, a set of esses, a lit
//! tunnel, a wide traffic-choked straight, a canyon squeeze, a closing sweep and
//! a finish arch. The car is an authored arcade model — instant acceleration,
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
pub mod controls;
pub mod diagnostics;
pub mod draw;
pub mod hud;
pub mod profile;
pub mod sim;
pub mod touch;
pub mod track;
pub mod tuning;

pub mod audio_cues;
pub mod debug_view;
pub mod render;

pub mod app;
pub mod capture;
pub mod script;

#[cfg(target_arch = "wasm32")]
pub mod web;
#[cfg(target_arch = "wasm32")]
pub use web::burnt_rubber_start;

pub use app::{build_burnt_rubber, BurntRubber};
pub use camera::{CameraPose, ChaseCamera};
pub use command::DriveCommand;
pub use profile::PlayProfile;
pub use sim::{RaceEvent, RacePhase, RaceSim};
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

/// The browser framebuffer the live app is configured for.
pub const WIDTH: u32 = 1280;
/// The browser framebuffer the live app is configured for.
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
        let track = Track::generate(DEFAULT_SEED, &Tuning::DEFAULT.course);
        assert!(
            (8_000.0..=10_500.0).contains(&track.length()),
            "the demo course is 8-10 km: {} m",
            track.length()
        );
        assert!(track.samples().len() > 4_000);
        assert_eq!(track.seed(), DEFAULT_SEED);
    }
}
