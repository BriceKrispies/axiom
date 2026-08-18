//! The hard-surface primitive kit. Ported from Claude-of-Duty
//! `src/weapons/geometry.js:51-357` — every `export function` between the
//! `normalizeAttributes` helper and the `Assembly` class. See
//! `docs/work-manifests/claude-of-duty-port/03-weapon-geometry-api.md` for
//! the fixed Rust API contract these are written against, and this module's
//! own files for the Three.js algorithms each one ports (`RoundedBoxGeometry`,
//! `LatheGeometry`, `SphereGeometry`, `TorusGeometry`, `ExtrudeGeometry` with
//! bevel and holes, `Earcut`, `PolyhedronGeometry`/`OctahedronGeometry`,
//! `BufferGeometryUtils.mergeVertices` — all MIT licensed, Three.js
//! authors).
//!
//! **The rule this whole kit exists to enforce** (`geometry.js:8-13`): there
//! is no such thing as a 90-degree edge on a real firearm. Every box carries
//! a 0.3-1.5 mm chamfer, every extrusion is bevelled, every tube end
//! crowned. A primitive here that "simplified" a chamfer away would defeat
//! the kit's entire purpose.
//!
//! `Geo`, `Assembly`, and `merge_all` are declared in sibling files
//! (`../geo.rs`, `../assembly.rs`, `../merge.rs`) from a concurrent port
//! pass against the same contract; this module only ever *uses* them,
//! through `super::{Geo, merge_all}`.

mod earcut;
mod extrude;
mod lathe;
mod octahedron;
mod parts;
mod rounded_box;
mod sphere;
mod torus;
mod xform;

pub use extrude::{extrude, round_rect, ExtrudeOpts};
pub(crate) use lathe::lathe_geometry;
pub use lathe::{lathe_z, rod_z, tube_z};
pub use parts::{knurl_band, mlok_slot, picatinny, screw, serrations, Axis, PicatinnyOpts};
pub use rounded_box::{blob, box_geo};
pub use sphere::dome;
pub(crate) use sphere::sphere_geometry;
pub use torus::ring;
