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
// retro FPS is now the `games/retro-fps` cartridge (`axiom-game-retro-fps`), not an in-crate
// module — the gallery is one of its HOSTS. Re-export its `#[wasm_bindgen]`
// `retro_fps_start` so the shared bundle still exports the `retro_fps_start` entry
// gallery.js boots (the re-export links the cartridge's wasm entry into the
// gallery cdylib). wasm-only, mirroring how the demo runs.
#[cfg(target_arch = "wasm32")]
pub use axiom_game_retro_fps::retro_fps_start;
pub mod stress_cubes;
pub mod growth;
pub mod zanzoban;
pub mod quintet;
pub mod physics_crucible;
pub mod harness;
pub mod forest_walk;
pub mod soccer_penalty;

/// Backend-comparison entry: render one demo three ways at once — WebGPU, WebGL2,
/// and Canvas 2D — into three canvases, from ONE wasm instance and ONE
/// deterministic sim. This is the no-iframe successor to the old gallery
/// triptych; a host (the workspace dev console) creates three canvases and calls
/// this. Only the engine-`App` 3D demos are comparable (`quintet` is a bespoke
/// Canvas 2D game, `retro_fps`/`soccer`/`netplay` build a `RunningApp` with their own
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
