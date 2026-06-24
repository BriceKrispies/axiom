//! # Axiom LevelGen — composed world recipe (feature module)
//!
//! The **composition tier** of the procedural-generation pivot (roadmap Phase 9).
//! [`LevelGenApi::generate`] folds the three domain generators into one [`World`]:
//! a terrain elevation field + an independent terrain moisture field, a biome map
//! classified from the two, and a scatter of placed objects — all keyed
//! deterministically by `(seed, address)`.
//!
//! ## Why a feature module
//! An engine module may never depend on another module (`allowed_modules = []`), so
//! no engine module could compose terrain + biome + placement. A **feature module**
//! (`kind = "feature-module"`) is the sanctioned exception: it may depend on
//! exactly the modules it lists — here `terrain`, `biome`, `placement` — the same
//! way `axiom-render-pipeline` composes the rotating-cube slice modules. It may be
//! depended on only by apps (or another feature module).
//!
//! ## It translates contracts, it does not leak them
//! Each domain module exposes one facade and keeps its result type behind it, so
//! `levelgen` cannot *name* `HeightField`/`BiomeMap`/`Placement`. It reads their
//! values through their methods and stores the read-outs in its own neutral
//! [`World`] — exactly the "apps/feature modules translate between module
//! contracts" rule. Branchless and 100%-covered like every module.
//!
//! ## Public surface
//! One facade: [`LevelGenApi`]. The `World` it returns is read through its methods.

mod levelgen_api;
mod world;

pub use levelgen_api::LevelGenApi;
