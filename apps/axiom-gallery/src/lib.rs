//! Axiom demo gallery — every browser/WASM demo merged into ONE composition-leaf
//! app crate.
//! Each demo keeps its own source tree under `src/<demo>/`, exposed here as a
//! public module. The merge rules that make nine independent crates coexist in
//! one crate:
//! * **Entry points are namespaced.** Every demo's wasm entry was `start`; in one
//!   crate the wasm-bindgen exports must be globally unique, so each is renamed
//!   `<demo>_start` (e.g. [`retro_fps::retro_fps_start`]). The gallery shell boots whichever
//!   the user picked from the single bundle. Every *other* exported symbol was
//!   already unique across the demos and is left as-is.
//! * **`crate::` is rebound.** Each demo was its own crate root; nested as a
//!   module, its internal `crate::…` paths become `crate::<demo>::…`.
//! * **Native agent drivers survive as feature-gated bins** (`retro-fps-agent`,
//!   `growth-agent`) and the physics report runner as `physics-crucible-report`.
//! Apps are composition leaves, exempt from the branchless and 100%-coverage
//! spine gates — which is what lets nine games with hand-written control flow live
//! here unchanged.

pub mod rotating_cube;
pub mod netplay;
pub mod retro_fps;
// retro FPS's `#[wasm_bindgen]` `retro_fps_start` surfaced at the crate root so the
// shared bundle exports the `retro_fps_start` entry gallery.js boots. wasm-only,
// mirroring how the demo runs.
#[cfg(target_arch = "wasm32")]
pub use retro_fps::retro_fps_start;
pub mod stress_cubes;
pub mod growth;
pub mod zanzoban;
pub mod quintet;
pub mod physics_crucible;
pub mod gravix;
// Gravix's `#[wasm_bindgen]` `gravix_start` surfaced at the crate root so the
// shared bundle exports the entry gallery.js boots (wasm-only, mirroring how the
// demo runs).
#[cfg(target_arch = "wasm32")]
pub use gravix::gravix_start;
pub mod harness;
pub mod forest_walk;
pub mod generia;
pub mod sports_physics_lab;
// The sports lab's `#[wasm_bindgen]` `sports_physics_lab_start` surfaced at the
// crate root so the shared bundle exports the entry gallery.js boots (wasm-only,
// mirroring how the demo runs).
#[cfg(target_arch = "wasm32")]
pub use sports_physics_lab::sports_physics_lab_start;

/// Build the rotating-cube demo's renderable core as a headless [`RunningApp`],
/// for the native capture harness (`axiom-shot`). The scene author
/// (`rotating_cube::rotating_cubes_app`) is browser-free; this exposes it as a
/// buildable core so the harness can render a real native frame of it.
pub fn rotating_cube_core() -> axiom::prelude::RunningApp {
    rotating_cube::rotating_cubes_app().build()
}

/// Build the stress-cubes demo's renderable core (an `N`-cube field) as a
/// headless [`RunningApp`] for the native capture harness.
pub fn stress_cubes_core(count: u32) -> axiom::prelude::RunningApp {
    stress_cubes::stress_cubes_app(count).build()
}

/// Backend-comparison entry: render one demo three ways at once — WebGPU, WebGL2,
/// and Canvas 2D — into three canvases, from ONE wasm instance and ONE
/// deterministic sim. This is the no-iframe successor to the old gallery
/// triptych; a host (the workspace dev console) creates three canvases and calls
/// this. Only the engine-`App` 3D demos are comparable (`quintet` is a bespoke
/// Canvas 2D game, `retro_fps`/`netplay` build a `RunningApp` with their own
/// input/relay wiring); an unknown or non-comparable `demo_id` is a no-op.
#[cfg(target_arch = "wasm32")]
#[wasm_bindgen::prelude::wasm_bindgen]
pub fn compare_start(demo_id: &str, canvas_a: &str, canvas_b: &str, canvas_c: &str) {
    console_error_panic_hook::set_once();
    let canvases = [canvas_a, canvas_b, canvas_c];
    match demo_id {
        "rotating-cube" => rotating_cube::rotating_cubes_app().run_compare(canvases),
        "stress-cubes" => stress_cubes::stress_cubes_app(2000).run_compare(canvases),
        "physics-crucible" => {
            physics_crucible::physics_crucible_app::crucible_app().run_compare(canvases)
        }
        _ => (),
    }
}
