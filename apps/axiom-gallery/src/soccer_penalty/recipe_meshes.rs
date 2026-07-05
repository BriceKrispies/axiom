//! Reusable **mesh recipe macros** for the soccer diorama's geometry.
//!
//! These replace the deleted hand-authored `penalty_meshes` [`MeshData`]
//! generators (`unit_cube` / `unit_sphere` / `unit_capsule`): every shape the
//! diorama draws is now a small [`RecipeGraph`] of mesh operators
//! (`axiom-proc-mesh`), baked by the render bridge and registered through the
//! ordinary `RunningApp::add_mesh_data` hook. The app builds no vertices of its
//! own.
//!
//! Each returns a **unit-extent** mesh (fits a 1×1×1 box centred at the origin —
//! except the capsule, whose XZ footprint is 0.8), so the render bridge scales it
//! per object exactly as it scaled the old hand meshes. The shape → mesh mapping
//! in `penalty_render_meshed::select_mesh` is unchanged: boxes for structure and
//! torsos, the rounded ball for the ball and head/hand joints, the capsule for
//! limbs.
//!
//! Recipe ids are allocated in the `700..` band.
//!
//! [`MeshData`]: axiom::prelude::MeshData

use axiom_proc_mesh::MeshOp;
use axiom_recipe::{Param, RecipeGraph, RecipeId, Scalar};

fn s(v: f32) -> Param {
    Param::scalar(Scalar::new(v))
}
fn i(v: u32) -> Param {
    Param::int(v)
}

/// Stable soccer mesh recipe ids.
pub mod ids {
    /// The axis-aligned box — structure, torsos, posts, boards, thin slabs.
    pub const BOX: u64 = 700;
    /// The rounded ball — the ball itself and the athletes' head/hand joints.
    pub const SPHERE: u64 = 701;
    /// The rounded limb tube — the athletes' arms and legs.
    pub const CAPSULE: u64 = 702;
    /// The softened (bevelled) box — the athletes' torsos, pelvises, and feet.
    pub const BEVEL_BOX: u64 = 703;
    /// Base of the reserved band for per-pose **continuous athlete body** recipes
    /// (`MetaSurface`, one per kit-material group), built inline from the live
    /// pose by [`crate::soccer_penalty::penalty_body`] rather than in
    /// [`catalog`]. Ids `710..` are allocated per group per author pass.
    pub const BODY_BASE: u64 = 710;
}

/// A unit cube (extent 1, centred at the origin), UV-projected — the shared shape
/// for every axis-aligned box the render bridge scales into a slab.
pub fn box_mesh() -> RecipeGraph {
    let mut g = RecipeGraph::new(RecipeId::from_raw(ids::BOX), 1);
    let cube = g.add(MeshOp::Cube as u16, vec![s(1.0)], vec![]);
    g.add(MeshOp::UVProject as u16, vec![s(0.5)], vec![cube]);
    g
}

/// A true unit sphere (extent 1): a UV sphere of radius 0.5, 12 rings × 16
/// segments — bounding box 1 × 1 × 1. Used for the ball and the athletes'
/// head/hand joints. This is the genuine `MeshOp::Sphere` primitive (added to
/// `axiom-proc-mesh` for exactly this), so the ball is round rather than a faceted
/// cube/cylinder approximation. Its unit bounding box keeps the render bridge's
/// radius→diameter scaling exact.
pub fn sphere_mesh() -> RecipeGraph {
    let mut g = RecipeGraph::new(RecipeId::from_raw(ids::SPHERE), 1);
    g.add(MeshOp::Sphere as u16, vec![s(0.5), i(12), i(16)], vec![]);
    g
}

/// A limb tube: a 12-segment capped cylinder of radius 0.4, height 1 (bounding
/// box 0.8 x 1.0 x 0.8), matching the old `unit_capsule`'s footprint so the render
/// bridge's `size.x / 0.8` scaling still fills a limb's box extents. No bevel:
/// `Bevel` would only shrink the tube toward its centroid (it does not round the
/// caps), throwing off that footprint.
pub fn capsule_mesh() -> RecipeGraph {
    let mut g = RecipeGraph::new(RecipeId::from_raw(ids::CAPSULE), 1);
    let cyl = g.add(MeshOp::Cylinder as u16, vec![s(0.4), s(1.0), i(16)], vec![]);
    g.add(MeshOp::UVProject as u16, vec![s(0.5)], vec![cyl]);
    g
}

/// A softened box: a unit cube pulled a small amount toward its centroid
/// (`Bevel[0.12]`) then UV-projected, so its edges read rounded rather than
/// hard-cut. Used for the athletes' torsos, pelvises, and feet — the boxy body
/// masses that should look modelled, not blocky. The bevel shrinks the mesh a
/// touch off its unit bbox, so a bevelled box renders slightly inset inside the
/// part's box extents (the intended softened look); structure boxes keep the
/// crisp [`box_mesh`].
pub fn bevel_box_mesh() -> RecipeGraph {
    let mut g = RecipeGraph::new(RecipeId::from_raw(ids::BEVEL_BOX), 1);
    let cube = g.add(MeshOp::Cube as u16, vec![s(1.0)], vec![]);
    let bevelled = g.add(MeshOp::Bevel as u16, vec![s(0.12)], vec![cube]);
    g.add(MeshOp::UVProject as u16, vec![s(0.5)], vec![bevelled]);
    g
}

/// Every soccer mesh recipe, paired with a stable name.
pub fn catalog() -> Vec<(&'static str, RecipeGraph)> {
    vec![
        ("box", box_mesh()),
        ("sphere", sphere_mesh()),
        ("capsule", capsule_mesh()),
        ("bevel_box", bevel_box_mesh()),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_mesh_recipe_validates() {
        for (name, recipe) in catalog() {
            assert_eq!(recipe.validate(), Ok(()), "{name} mesh recipe is a valid DAG");
        }
    }

    #[test]
    fn mesh_ids_are_unique() {
        let mut ids: Vec<u64> = catalog().iter().map(|(_, r)| r.id().raw()).collect();
        ids.sort_unstable();
        ids.dedup();
        assert_eq!(ids.len(), catalog().len());
    }
}
