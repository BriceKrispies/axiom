//! Axiom demo gallery — every browser/WASM demo merged into ONE composition-leaf
//! app crate.
//!
//! Each demo keeps its own source tree under `src/<demo>/`, exposed here as a
//! public module. The merge rules that make nine independent crates coexist in
//! one crate:
//!
//! * **Entry points are namespaced.** Every demo's wasm entry was `start`; in one
//!   crate the wasm-bindgen exports must be globally unique, so each is renamed
//!   `<demo>_start` (e.g. [`retro_fps::retro_fps_start`]). The gallery shell boots whichever
//!   the user picked from the single bundle. Every *other* exported symbol was
//!   already unique across the demos and is left as-is.
//! * **`crate::` is rebound.** Each demo was its own crate root; nested as a
//!   module, its internal `crate::…` paths become `crate::<demo>::…`.
//! * **Native agent drivers survive as feature-gated bins** (`retro-fps-agent`,
//!   `growth-agent`) and the physics report runner as `physics-crucible-report`.
//!
//! Apps are composition leaves, exempt from the branchless and 100%-coverage
//! spine gates — which is what lets nine games with hand-written control flow live
//! here unchanged.

pub mod rotating_cube;
pub mod netplay;
pub mod retro_fps;
pub mod stress_cubes;
pub mod growth;
pub mod roomed_puzzle;
pub mod quintet;
pub mod physics_crucible;
pub mod harness;
