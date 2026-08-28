//! Ported from Claude-of-Duty `src/world/props.js` (994 lines) — the prop
//! prototype library. Every prop is a small assembly of chamfered boxes,
//! tubes, cloth grids and noise-deformed rocks, merged into ONE geometry
//! (via [`pb::PB`], `props.js`'s own local part accumulator) and registered
//! as an instanced prototype through [`registry::register_props`].
//! Placement (rotation/scale/tint variation, `LOOSE`'s jitter) is the
//! Assembler's own job (`crate::world::assembler`); this module only
//! decides what things look like.
//!
//! Split into one file per registry section (matching `props.js`'s own
//! banner comments — containers/cover/furniture/services/debris/vegetation/
//! signage/vehicles), plus [`mesh`] for the low-level, non-`PB` geometry
//! builders (`autoEdgeWear`, `sackGeometry`, `warpGeometry`, `pockGeometry`)
//! and [`pb`] for the accumulator itself.
//!
//! Mask convention as everywhere else: `r` = edge wear, `g` = grime, `b` =
//! extra AO, multiplied per instance by `instanceColor` so no two crates
//! weather alike (`props.js:25-27`; the convention itself is documented in
//! full at [`crate::world::masks`]).

mod containers;
mod cover;
mod debris;
mod furniture;
mod mesh;
mod pb;
mod registry;
mod services;
mod signage;
mod vegetation;
mod vehicles;

pub use registry::{register_props, RegisteredProto};

// `burnt_car` (`export function burntCar`) is called by `dressing.js`, not by
// `registerProps`. This module deferred its re-export until a real caller
// existed rather than leave an always-unused `pub use`; `world::dressing` is
// now that caller, so here it is — `pub(crate)`, because the caller is in this
// crate and the public surface does not need to grow for it.
pub(crate) use vehicles::burnt_car;

// `auto_edge_wear` (`export function autoEdgeWear`) is still exported in the
// source but only ever called from within
// `props::pb`/`props::cover`/`props::debris`/`props::services`, so it stays
// unexported here on the same rule.
