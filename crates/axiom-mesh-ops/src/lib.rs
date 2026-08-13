//! # Axiom Mesh-Ops — the deterministic geometry library (layer)
//!
//! `mesh-ops` constructs [`axiom_mesh::Mesh`] values. Every operator is a pure
//! function from explicit input data to a validated mesh.
//!
//! ## The operator contract
//!
//! ```text
//! explicit deterministic input data  ->  MeshResult<Mesh>
//! ```
//!
//! No ambient state, no global state, no wall clock, no unseeded randomness, no
//! hidden resource lookup, no callbacks, no scene access, no GPU access. An
//! operator handed the same inputs twice produces byte-identical output, which
//! is what makes generated geometry replayable and cacheable by digest.
//!
//! ## Operator families
//!
//! - **Primitives** — triangle, quad, box, cube, UV sphere, icosphere, cylinder,
//!   cone, frustum, capsule, torus, disk, annulus, grid, rounded box.
//! - **Constructive** — polygon triangulation, extrusion, sweep along a curve,
//!   loft between sections, revolution/lathe.
//! - **Sampled-data surfaces** — parametric surface tessellation, heightfield to
//!   mesh, marching-cubes implicit-field extraction.
//! - **Refinement** — midpoint and Loop subdivision, quadric-error
//!   simplification.
//!
//! ## Why semantic generators do not live here
//!
//! There is no `road`, `tree`, `building`, `car`, or `terrain` in this layer,
//! and there must never be. A road is a sweep of a particular profile along a
//! particular curve; a tree is a tapered sweep plus some lofted crowns. Those
//! are *compositions*, and the composition is where the domain meaning lives —
//! in an app or a module. Admitting one semantic generator here would make this
//! layer the junk drawer every future domain reaches into.
//!
//! ## Why recipe semantics stay above it
//!
//! `axiom-proc-mesh` owns the recipe graph: operator codes, `Param` words,
//! per-node entropy, graph baking. That is a *data-driven front end* to
//! geometry, not geometry itself. This layer exposes the algorithms as ordinary
//! typed functions so a recipe interpreter, an importer, an app, and a test can
//! all reach the same code without any of them needing a `RecipeGraph`.

mod cap_policy;
mod profile;
mod tessellation;

pub use cap_policy::CapPolicy;
pub use profile::{Profile, ProfileWinding, PROFILE_EPSILON};
pub use tessellation::{
    DetailBudget, Rings, Samples, Segments, Subdivisions, MAX_RINGS, MAX_SAMPLES, MAX_SEGMENTS,
    MAX_SUBDIVISIONS,
};

mod heightfield;
mod implicit_surface;
mod marching_cubes_tables;
mod surface_tessellation;

pub use heightfield::{
    heightfield_mesh, HeightfieldOptions, HeightfieldSamples, TriangleDiagonal,
};
pub use implicit_surface::{
    implicit_surface_mesh, ImplicitSurfaceOptions, IsoValue, ScalarField,
};
pub use surface_tessellation::{tessellate_surface, SurfaceGrid};

mod primitive_box;
mod primitive_disk;
mod primitive_grid;
mod primitive_quad;
mod primitive_triangle;

pub use primitive_box::{box_mesh, cube};
pub use primitive_disk::{annulus, disk};
pub use primitive_grid::grid;
pub use primitive_quad::quad;
pub use primitive_triangle::triangle;

mod extrude;
mod polygon_triangulation;

pub use extrude::extrude;
pub use polygon_triangulation::triangulate_profile;

mod primitive_capsule;
mod primitive_cone;
mod primitive_cylinder;
mod primitive_frustum;
mod primitive_icosphere;
mod primitive_sphere;
mod primitive_torus;

pub use primitive_capsule::capsule;
pub use primitive_cone::cone;
pub use primitive_cylinder::cylinder;
pub use primitive_frustum::frustum;
pub use primitive_icosphere::icosphere;
pub use primitive_sphere::uv_sphere;
pub use primitive_torus::torus;

mod simplification;
mod subdivision;

pub use simplification::{simplify_quadric, SimplifyTarget};
pub use subdivision::{subdivide_loop, subdivide_midpoint};

mod loft;
mod primitive_rounded_box;
mod revolve;
mod sweep;
mod sweep_frames;

pub use loft::{loft, LoftOptions, LoftSection};
pub use primitive_rounded_box::rounded_box;
pub use revolve::revolve;
pub use sweep::{sweep, SweepOptions};
pub use sweep_frames::{parallel_transport_frames, SweepFrame};
