//! The detail ladder: one shape at several densities, standing next to itself.
//!
//! Two comparisons live here, and they answer different questions.
//!
//! 1. **Generated density.** Four icospheres at subdivision 0, 1, 2 and 3. The
//!    silhouette is the same sphere every time; only the tessellation changes.
//!    This is what "the same authored intent at a different budget" looks like.
//! 2. **Refinement versus reduction.** One UV sphere put through
//!    `subdivide_loop` and the *same* UV sphere put through `simplify_quadric`,
//!    with the untouched original between them. Loop subdivision smooths as it
//!    refines (the limit surface pulls inward), quadric simplification collapses
//!    edges by error and keeps the silhouette. Side by side, at the same place
//!    in the row, the difference is a thing you can see rather than a claim.
//!
//! The base sphere for the second comparison is deliberately *not* the icosphere
//! from the first: a UV sphere has poles and a seam, which is where a
//! simplifier's edge-collapse ordering and a subdivider's valence handling both
//! actually get tested.

use axiom_kernel::Ratio;
use axiom_mesh::{Mesh, MeshResult};
use axiom_mesh_ops::{
    icosphere, simplify_quadric, subdivide_loop, uv_sphere, Rings, Segments, SimplifyTarget,
    Subdivisions,
};

use crate::quantities::{meters, ratio};
use crate::variant::DetailParams;

/// The four generated-density rungs, by subdivision level.
pub const ICOSPHERE_RUNGS: [u32; 4] = [0, 1, 2, 3];

/// The radius every rung is built at, so only the tessellation differs.
const RUNG_RADIUS: f32 = 1.6;

/// The fraction of the base sphere's triangles the quadric simplifier keeps.
const SIMPLIFY_FRACTION: f32 = 0.22;

/// One icosphere rung.
pub fn icosphere_rung(level: u32) -> MeshResult<Mesh> {
    Subdivisions::new(level).and_then(|levels| icosphere(meters(RUNG_RADIUS), levels))
}

/// The untouched UV sphere the refine/reduce pair is measured against.
pub fn ladder_base(params: DetailParams) -> MeshResult<Mesh> {
    Rings::new(params.sphere_rings)
        .and_then(|rings| Segments::new(params.sphere_segments).map(|segs| (rings, segs)))
        .and_then(|(rings, segs)| uv_sphere(meters(RUNG_RADIUS), rings, segs))
}

/// The base sphere refined by Loop subdivision.
pub fn ladder_refined(params: DetailParams) -> MeshResult<Mesh> {
    ladder_base(params)
        .and_then(|base| Subdivisions::new(params.ladder_levels).map(|levels| (base, levels)))
        .and_then(|(base, levels)| subdivide_loop(&base, levels))
}

/// The base sphere reduced by quadric-error edge collapse.
pub fn ladder_reduced(params: DetailParams) -> MeshResult<Mesh> {
    ladder_base(params).and_then(|base| {
        simplify_quadric(&base, SimplifyTarget::Fraction(simplify_fraction()))
    })
}

/// The keep-fraction, as a validated ratio.
fn simplify_fraction() -> Ratio {
    ratio(SIMPLIFY_FRACTION)
}
