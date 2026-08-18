//! The collision world: ported from Claude-of-Duty `src/physics/math.js`,
//! `src/physics/surfaces.js` and `src/physics/bvh.js`.
//!
//! | this module         | source                     |
//! |----------------------|----------------------------|
//! | [`math`]             | `src/physics/math.js`      |
//! | [`surfaces`]          | `src/physics/surfaces.js`  |
//! | [`bvh`]                | `src/physics/bvh.js`       |
//!
//! `math` is an allocation-free (here: allocation-*unnecessary* — see its own
//! doc comment) geometric kernel: ray/AABB/triangle/segment primitives in
//! `f64`, matching the JavaScript's own number type exactly. `surfaces`
//! carries the physical-response table (penetration depth, friction,
//! restitution, ...) for the twelve-entry surface taxonomy that already lives
//! as `crate::world::palette::Surface`, plus the collision layer/mask
//! bitflags. `bvh` is the binned-SAH bounding volume hierarchy over a
//! triangle soup — [`bvh::StaticWorld`] — and its queries: closest-hit and
//! any-hit raycasts, an AABB range query, and the two capsule queries
//! (`overlap_capsule` for resting penetration, `sweep_capsule` for continuous
//! motion via conservative advancement).
//!
//! This is a pure algorithm over flat typed arrays with no rendering contact,
//! which is what makes it checkable by golden capture rather than merely
//! plausible — see `tests/physics_port.rs` and
//! `docs/work-manifests/claude-of-duty-port/notes/physics.md`.
//!
//! What is *not* ported here: `bakeMesh`/`StaticWorld.addMesh`
//! (`bvh.js:104-125, 836-933`), which flatten a live `THREE.Mesh` — see
//! [`bvh`]'s module doc comment for why and what a future mesh-baking arm
//! should reproduce.

pub mod bvh;
pub mod math;
pub mod surfaces;
