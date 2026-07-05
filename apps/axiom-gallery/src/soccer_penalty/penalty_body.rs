//! Continuous athlete bodies — skin a set of posed box parts into one smooth
//! surface via the engine's `MetaSurface` mesh operator.
//!
//! The athletes were drawn as a *union of disjoint primitives* (box torso,
//! capsule limbs, sphere head — plus a sphere jammed into every joint to hide the
//! gaps). That "box-man with ball joints" look is a property of the
//! representation, not of tuning: `axiom-figure` is an articulated *box*-figure,
//! one primitive per part. This module instead treats each posed part as a
//! **capsule field primitive** and hands the group to
//! [`axiom_proc_mesh::MeshOp::MetaSurface`], whose metaball smooth-union +
//! marching cubes fuses the parts into one continuous, tapered body.
//!
//! The app groups a figure's parts **by kit material** (jersey, shorts, skin,
//! socks) and bakes one surface per group, so the existing colour story is kept
//! and the only seams fall at real kit boundaries. Composition lives here, in an
//! app: the isolated `axiom-proc-mesh` layer owns the primitive; the soccer app
//! owns which capsules make a soccer player.

use axiom_math::{Quat, Vec3};
use axiom_proc_mesh::MeshOp;
use axiom_recipe::{Param, RecipeGraph, RecipeId, Scalar};

/// One posed box part of an athlete, already resolved to world space by the
/// render plan.
#[derive(Clone, Copy, Debug)]
pub struct BodyPart {
    pub position: Vec3,
    pub rotation: Quat,
    pub size: Vec3,
}

/// Target grid cell (world units) — finer than the thinnest limb so arms and
/// shins resolve rather than vanishing between samples.
const TARGET_CELL: f32 = 0.03;
/// A capsule's radius as a fraction of the box cross-section's average
/// half-extent. The smooth-union already fattens joints, so the base stays lean.
const LIMB_FILL: f32 = 0.92;
/// The iso level: 0 is the field's own inflated-radius surface.
const ISO: f32 = 0.0;
/// The smooth-union blend radius, in the athletes' world units — how far apart
/// two parts' surfaces fuse at a joint. This is the app's scale decision (a
/// soccer player is ~1.8 units tall with ~0.1-radius limbs), passed into the
/// domain-free `MetaSurface` op; the op itself assumes nothing about scale.
const BLEND_RADIUS: f32 = 0.15;
/// Grid-resolution clamps: keep thin limbs resolvable without blowing the vertex
/// budget the `MetaSurface` op enforces.
const MIN_RES: u32 = 20;
const MAX_RES: u32 = 58;

/// The capsule field primitive `[ax, ay, az, bx, by, bz, r]` for a posed part: a
/// segment along the part's local Y (its length/bone axis) inflated to fill the
/// box cross-section. A short part (torso, head, foot) yields a near-spherical
/// capsule; a long part (limb) yields a tapered tube — and adjacent capsules fuse
/// at the shared joint.
fn part_capsule(part: &BodyPart) -> [f32; 7] {
    let half = part.size.y * 0.5;
    let dir = part.rotation.rotate(Vec3::new(0.0, half, 0.0));
    let a = part.position.subtract(dir);
    let b = part.position.add(dir);
    let r = (part.size.x + part.size.z) * 0.25 * LIMB_FILL;
    [a.x, a.y, a.z, b.x, b.y, b.z, r]
}

/// The grid resolution for a group: dice its largest span into ~`TARGET_CELL`
/// cells, clamped so the vertex count stays bounded.
fn group_res(caps: &[[f32; 7]]) -> u32 {
    let mut lo = Vec3::new(f32::MAX, f32::MAX, f32::MAX);
    let mut hi = Vec3::new(f32::MIN, f32::MIN, f32::MIN);
    for c in caps {
        let r = c[6];
        for &(x, y, z) in &[(c[0], c[1], c[2]), (c[3], c[4], c[5])] {
            lo = Vec3::new(lo.x.min(x - r), lo.y.min(y - r), lo.z.min(z - r));
            hi = Vec3::new(hi.x.max(x + r), hi.y.max(y + r), hi.z.max(z + r));
        }
    }
    let span = hi.subtract(lo);
    let max_dim = span.x.max(span.y).max(span.z);
    ((max_dim / TARGET_CELL).round() as u32).clamp(MIN_RES, MAX_RES)
}

/// Build the `MetaSurface` recipe that skins `parts` into one continuous surface.
/// `recipe_id` must be unique among the app's registered meshes. `parts` must be
/// non-empty (a material group always has at least one part).
pub fn body_recipe(recipe_id: u64, parts: &[BodyPart]) -> RecipeGraph {
    let caps: Vec<[f32; 7]> = parts.iter().map(part_capsule).collect();
    let res = group_res(&caps);
    let mut params = vec![Param::scalar(Scalar::new(ISO)), Param::int(res), Param::scalar(Scalar::new(BLEND_RADIUS))];
    for c in &caps {
        for &v in c {
            params.push(Param::scalar(Scalar::new(v)));
        }
    }
    let mut g = RecipeGraph::new(RecipeId::from_raw(recipe_id), 1);
    g.add(MeshOp::MetaSurface as u16, params, vec![]);
    g
}

#[cfg(test)]
mod tests {
    use super::*;
    use axiom_proc_mesh::ProcMeshApi;

    fn part(y: f32, sy: f32, sxz: f32) -> BodyPart {
        BodyPart {
            position: Vec3::new(0.0, y, 0.0),
            rotation: Quat::IDENTITY,
            size: Vec3::new(sxz, sy, sxz),
        }
    }

    #[test]
    fn a_stacked_torso_and_head_bake_one_continuous_body() {
        // A torso capsule with a head above it, sharing a material group: the
        // MetaSurface fuses them into a single connected mesh.
        let parts = vec![part(1.0, 0.6, 0.3), part(1.45, 0.2, 0.22)];
        let recipe = body_recipe(710, &parts);
        let mesh = ProcMeshApi::new().bake(&recipe, 0).expect("body recipe bakes");
        assert!(mesh.triangle_count() > 0);
        // The surface spans from below the torso to above the head.
        let ys: Vec<f32> = mesh.positions().iter().map(|p| p.y).collect();
        let min = ys.iter().copied().fold(f32::MAX, f32::min);
        let max = ys.iter().copied().fold(f32::MIN, f32::max);
        assert!(min < 0.8 && max > 1.5, "body spans torso..head, got {min}..{max}");
    }

    #[test]
    fn a_rotated_limb_capsule_follows_its_bone() {
        // A limb rotated 90° about Z lies along X, not Y.
        let limb = BodyPart {
            position: Vec3::ZERO,
            rotation: Quat::from_axis_angle(Vec3::UNIT_Z, core::f32::consts::FRAC_PI_2).unwrap(),
            size: Vec3::new(0.15, 1.0, 0.15),
        };
        let cap = part_capsule(&limb);
        // Endpoints separated along X (|ax - bx| ≈ length 1.0), not Y.
        assert!((cap[0] - cap[3]).abs() > 0.9, "capsule runs along X after the rotation");
        assert!((cap[1] - cap[4]).abs() < 0.1);
    }

    #[test]
    fn resolution_scales_with_group_extent_and_clamps() {
        // A tiny group clamps up to MIN_RES; a huge group clamps down to MAX_RES.
        let tiny = [part_capsule(&part(0.0, 0.05, 0.05))];
        assert_eq!(group_res(&tiny), MIN_RES);
        let huge = [part_capsule(&part(0.0, 100.0, 1.0))];
        assert_eq!(group_res(&huge), MAX_RES);
    }
}
