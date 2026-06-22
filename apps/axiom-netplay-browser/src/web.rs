//! The live `wasm32` arm: render the two cubes at the positions the page sets.
//! Never compiled on native.
//!
//! All networking — plus client-side **prediction** and **interpolation** — lives
//! in the page's JavaScript over the `@axiom/client` SDK. Each animation frame the
//! page computes where the two cubes should be drawn (its own cube predicted, the
//! other interpolated from snapshots) and pushes them via [`set_positions`]. This
//! module owns a real engine instance and renders those absolute positions by
//! feeding the delta-to-target into `tick_with`.

use std::cell::Cell;

use axiom_windowing::WindowingApi;
use wasm_bindgen::prelude::*;

use super::{build_netplay_app, inputs_to_targets, CANVAS_ID, INITIAL_POSITIONS};

thread_local! {
    /// The positions `[p0x, p0y, p1x, p1y]` the page wants drawn this frame.
    /// Seeded to spawn so the scene is correct before the first update.
    static TARGET: Cell<[f32; 4]> = const { Cell::new(INITIAL_POSITIONS) };
}

/// Set where the two cubes should be drawn. Called from the page's JavaScript
/// every animation frame with the predicted (own) + interpolated (other)
/// positions.
#[wasm_bindgen]
pub fn set_positions(p0x: f32, p0y: f32, p1x: f32, p1y: f32) {
    TARGET.with(|t| t.set([p0x, p0y, p1x, p1y]));
}

/// The browser entry: build the engine and start the presentation loop. The page
/// calls this once, after setting up its `@axiom/client` connection.
#[wasm_bindgen]
pub fn start() {
    console_error_panic_hook::set_once();
    run_loop();
}

/// Drive this browser's own engine, each frame moving the cubes to the latest
/// positions the page set (no networking, no authoritative simulation here).
fn run_loop() {
    let mut running = build_netplay_app();
    let (vertices, indices) = running.mesh_vertex_stream();
    let max_instances = running.renderable_count() as u32;

    let mut windowing = WindowingApi::new();
    windowing
        .configure_surface(800, 600)
        .expect("surface dimensions are valid");

    // The cubes' currently-rendered positions; each frame we move them to target.
    let mut current = INITIAL_POSITIONS;

    let _ = windowing.run_web(CANVAS_ID, vertices, indices, max_instances, move |tick| {
        let target = TARGET.with(|t| t.get());
        let inputs = inputs_to_targets(current, target);
        current = target;
        let outcome = running.tick_with(tick, &inputs);
        (
            outcome.clear_color(),
            outcome.instance_floats(),
            outcome.draws().len() as u32,
        )
    });
}
