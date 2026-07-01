//! # Axiom Hydrology — graph-hydrology field solvers (layer)
//!
//! `hydrology` is the engine's drainage-field primitive: the pure, deterministic,
//! branchless solvers that turn the `geosphere` region-adjacency graph plus a
//! per-region elevation field into the fields a planet's water cycle needs — a
//! pit-filled drainage surface, steepest-descent receivers, flow accumulation,
//! stream-power erosion, and multi-source ocean distance. Given the same graph
//! and inputs the outputs are byte-identical on every run and platform.
//!
//! ## What it is, and is not
//! - It is **pure graph-plus-scalar transforms**: `(graph, fields) → fields`. It
//!   owns **no** world state, no genome, no [`Stage`] orchestration — the app (or
//!   a future planet-generation feature-module) owns the fields and calls these
//!   functions on them. The growth app's worldgen stages (`priority_flood`,
//!   `rivers`, `erosion`, `moisture`) become thin wrappers over this layer.
//! - It is **neutral**: it knows nothing of biomes, genomes, wind, or gameplay.
//!
//! ## Why a layer, depending on geosphere + kernel
//! Several generators need the same drainage math, and an engine **module** may
//! depend only on **layers** (never on another module) — so the shared hydrology
//! primitive is a layer a planetgen module can build on. It genuinely uses
//! **geosphere** (every solver traverses a [`axiom_geosphere::RegionGraph`] via
//! `neighbours_of`, keyed by [`axiom_geosphere::RegionId`]) and **kernel** (the
//! elevation field is [`axiom_kernel::Meters`] and flow is
//! [`axiom_kernel::Ratio`], so no naked scalar reaches the public API). The
//! algorithms use no `Vec3`/`Mat`, so `math` is **not** a dependency — declaring
//! it would be a ceremonial edge the Layer Law forbids.
//!
//! ## Branchless drainage — heaps and queues become wavefronts
//! The classic implementations lean on a `BinaryHeap` (priority flood) and a
//! `VecDeque` (BFS). Axiom's spine is branchless, so both become **bounded
//! wavefront relaxation**, the sanctioned substitute (see `docs/unbranching.md`
//! and the `axiom-grid` distance field):
//! - [`pit_fill`] — a monotone `max(own, least neighbour)` relaxation seeded at
//!   outlets, converging to the priority-flood surface with no heap.
//! - [`ocean_distance`] — a multi-source BFS as `region_count` `min(self, 1 +
//!   least neighbour)` passes, saturating at [`HopDistance::UNREACHABLE`].
//! - [`compute_receivers`] / [`flow_accumulation`] — an `(elevation, index)`
//!   argmin `fold` and a sorted, deterministic downstream scatter.
//! - [`stream_power_erosion`] — slope-proportional incision folded per pass.
//!
//! ## Public surface
//! - [`pit_fill`] — in-place priority-flood drainage surface.
//! - [`compute_receivers`] — per-region steepest-descent receiver ([`RegionId`]).
//! - [`flow_accumulation`] — downstream unit-rainfall accumulation ([`Ratio`]).
//! - [`stream_power_erosion`] — iterative slope-proportional incision.
//! - [`ocean_distance`] — multi-source graph-hop distance field.
//! - [`HopDistance`] — the hop-count the ocean-distance field traffics in.
//!
//! [`Stage`]: https://docs.rs/ "(app-side orchestration; not part of this layer)"
//! [`RegionId`]: axiom_geosphere::RegionId
//! [`Ratio`]: axiom_kernel::Ratio

mod drainage;
mod erosion;
mod hop_distance;
mod ocean_distance;
mod pit_fill;

#[cfg(test)]
mod test_graphs;

pub use drainage::{compute_receivers, flow_accumulation};
pub use erosion::stream_power_erosion;
pub use hop_distance::HopDistance;
pub use ocean_distance::ocean_distance;
pub use pit_fill::pit_fill;
