//! # Axiom Geosphere — geodesic-icosphere topology + region graph (layer)
//!
//! `geosphere` is the engine's spherical-topology primitive: a geodesic
//! **icosphere** (an icosahedron subdivided on a barycentric lattice, every
//! vertex normalized onto the unit sphere, shared edge/corner vertices
//! deduplicated into stable region indices) and its **dual region-adjacency
//! graph** in compressed-sparse-row (CSR) form, plus a ring/manifold validator.
//! For a given subdivision level the emitted topology is byte-identical on every
//! run and platform.
//!
//! ## What it is, and is not
//! - It builds a **fixed topology**: unit-sphere region [`sites`](Icosphere::sites)
//!   and outward-CCW [`triangles`](Icosphere::triangles) of region indices, and
//!   the [`RegionGraph`] of which regions share an edge. It hangs **no** scalar
//!   fields (elevation, moisture, plates) off the regions — those belong to the
//!   consumer that owns the *content*, not the topology.
//! - It is **neutral**: it knows nothing of planets, hydrology, or gameplay. A
//!   hydrology layer or a planet-generation module consumes the CSR graph; this
//!   layer never depends upward.
//!
//! ## Why a layer, depending on math
//! Several generators need the same spherical topology, and an engine **module**
//! may depend only on **layers** (never on another module) — so the shared
//! topology primitive is a layer a hydrology layer / planetgen module can build
//! on. It genuinely uses **math**: every region site is an [`axiom_math::Vec3`] on
//! the unit sphere, and the subdivision, outward-orientation and unit-projection
//! math is all `Vec3` arithmetic — so `depends_on = ["math"]`.
//!
//! ## Public surface
//! - [`Icosphere`] + [`build_icosphere`] — the topology and its builder, with
//!   [`subdivisions_for_target`] to pick a subdivision level for a region-count.
//! - [`RegionGraph`] + [`build_region_graph`] — the dual CSR adjacency graph.
//! - [`RegionId`] — the region-index newtype the graph and validator traffic in.
//! - [`RingReport`] + [`validate_region_rings`] — the closed-manifold check.

mod icosphere;
mod ids;
mod region_graph;
mod ring_validation;

pub use icosphere::{build_icosphere, subdivisions_for_target, Icosphere};
pub use ids::RegionId;
pub use region_graph::{build_region_graph, RegionGraph};
pub use ring_validation::{validate_region_rings, RingReport};
