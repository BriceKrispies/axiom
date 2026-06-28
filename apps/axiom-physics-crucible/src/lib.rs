//! # Axiom Physics Crucible — proof app
//!
//! A rendered, deterministic *physics proving room*. It is a composition leaf that
//! drives the `axiom-physics` engine module through its public `PhysicsApi` facade
//! across six scripted stations, translates every physics snapshot into renderable
//! debug geometry, and runs a hidden replay world in lock-step to make determinism
//! visible.
//!
//! ## The six stations
//! 1. **Body Bay** — static / dynamic / kinematic / disabled body kinds.
//! 2. **Contact Bay** — sphere/plane, sphere/sphere, sphere/box, box/plane contacts.
//! 3. **Material Bay** — a restitution rebound ladder + mass/impulse response.
//! 4. **Query Bay** — exact raycast (hit / miss / through-trigger) + overlap sphere.
//! 5. **Stress Bay** — a deterministic pile exercising the broad phase + solver.
//! 6. **Replay Bay** — the two-world determinism proof (and divergence detection).
//!
//! ## Architecture (the laws this app respects)
//! Physics and the renderer are isolated modules; **this app owns every boundary**
//! between them ([`physics_to_render`], [`debug_geometry`], [`debug_overlay`]).
//! Physics never imports a renderer type and the renderer never imports a physics
//! type. Because `axiom-physics` exposes a single facade, its snapshot / record /
//! contact / material types are unnameable here, so [`physics_crucible_app::CrucibleWorld`]
//! reads them only as inferred locals and projects them into the app-owned value
//! types in [`crucible_report`]. As a composition leaf, the app is exempt from the
//! branchless and 100%-coverage spine gates, but it ships with the tests its
//! behaviour warrants (every station is covered).
//!
//! ## Rendering note
//! The Axiom umbrella `App` exposes no per-frame external hook, and `PhysicsApi`
//! exposes no teleport, so the crucible pre-simulates deterministically to a hero
//! step and renders a faithful static frame of a *real* simulation
//! ([`build_physics_crucible`]). Per-step motion, contacts, queries, and replay are
//! proven by the headless harness ([`run_report`]) and the test suite. See
//! `README.md` for the exact renderer/physics gaps this works around.

pub mod body_bay;
pub mod contact_bay;
pub mod crucible_camera;
pub mod crucible_report;
pub mod crucible_scenario;
pub mod crucible_station;
pub mod debug_geometry;
pub mod debug_overlay;
pub mod material_bay;
pub mod physics_crucible_app;
pub mod physics_to_render;
pub mod query_bay;
pub mod replay_bay;
pub mod stress_bay;

use crate::crucible_scenario::Station;

// The browser/WASM live arm: drives the windowing render loop, stepping physics
// and re-authoring the scene each frame so the bodies fall, bounce, and pile on
// screen — and turns keyboard/keypad input into live "kick"/"reset" actions.
// Confined to wasm32, so native builds never compile the platform glue.
#[cfg(target_arch = "wasm32")]
mod web;

pub use crate::crucible_report::CrucibleReport;
pub use crate::physics_crucible_app::{build_physics_crucible, Crucible};

/// The six stations, in canonical order. The renderer, the headless harness, and
/// the report all drive this same list.
pub fn all_stations() -> Vec<Box<dyn Station>> {
    vec![
        Box::new(body_bay::BodyBay),
        Box::new(contact_bay::ContactBay),
        Box::new(material_bay::MaterialBay),
        Box::new(query_bay::QueryBay),
        Box::new(stress_bay::StressBay),
        Box::new(replay_bay::ReplayBay),
    ]
}

/// Build the full crucible, run every station's scripted scenario to completion,
/// and render the deterministic report as text (the headless entry point).
pub fn run_report() -> String {
    let mut crucible = Crucible::new(all_stations());
    crucible.run();
    let report = crucible.report();
    let mut out = String::from("Axiom Physics Crucible — deterministic proof run\n");
    out.push_str(&format!("stations: {}\n", all_stations().len()));
    out.push_str(&format!("steps:    0..={}\n", crucible.steps_run().saturating_sub(1)));
    out.push_str(&report.render());
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn all_stations_are_present_in_canonical_order() {
        let stations = all_stations();
        assert_eq!(stations.len(), 6);
        let ids: Vec<_> = stations.iter().map(|s| s.id()).collect();
        assert_eq!(ids, crate::crucible_station::CrucibleStation::ALL.to_vec());
    }

    #[test]
    fn run_report_is_deterministic_and_reports_a_match() {
        let a = run_report();
        let b = run_report();
        assert_eq!(a, b, "the headless report must be byte-reproducible");
        assert!(a.contains("replay_match:           true"));
        assert!(a.contains("stations: 6"));
    }

    #[test]
    fn the_full_crucible_keeps_both_worlds_in_sync() {
        let mut crucible = Crucible::new(all_stations());
        crucible.run();
        assert!(crucible.replay_matches());
        // Every station's bodies exist in the visible world.
        assert!(crucible.visible().bodies().len() > 20);
    }
}
