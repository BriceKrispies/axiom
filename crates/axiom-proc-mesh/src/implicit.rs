//! **MetaSurface** — skin a skeleton of capsules into one continuous surface.
//!
//! The field is a metaball **smooth-union** of per-capsule signed distances; the
//! zero-(or `iso`-)level set is polygonised by **marching cubes** over a bounded
//! grid, with per-vertex normals read from the field gradient (so the surface
//! shades smoothly without vertex welding). A sphere is a degenerate capsule
//! (`a == b`), so adjacent capsules fuse into one connected surface instead of a
//! union of disjoint primitives. The op is domain-free: the caller supplies the
//! capsule skeleton and the field parameters (`iso`, `res`, blend radius `k`) in
//! its own units — the op assumes nothing about scale. Every step is a data
//! transform (iterator adapters + `const`-table lookups): no control-flow branch.

use axiom_math::{Vec2, Vec3};
use axiom_proc_core::NodeEval;

use crate::mc_tables::{MC_CORNER_OFFSET, MC_EDGE_CORNERS, MC_TRI_TABLE};
use crate::mesh_buffer::MeshBuffer;

/// Numeric floor for guarded divisions (segment length, edge interpolation,
/// the caller's blend radius).
const EPS: f32 = 1.0e-6;
/// The grid subdivision clamp. A too-fine request is clamped here, never honoured
/// unbounded; the `MeshBuffer` vertex cap is the final backstop.
const MIN_RES: u32 = 2;
const MAX_RES: u32 = 64;
/// Central-difference step for the gradient normal.
const GRAD_H: f32 = 1.0e-3;

/// One field primitive: a capsule (a line segment `a`→`b` inflated by `radius`).
/// A sphere is the degenerate `a == b`.
#[derive(Clone, Copy, Debug)]
struct Capsule {
    a: Vec3,
    b: Vec3,
    radius: f32,
}

/// Signed distance from `p` to a capsule (negative inside). The segment
/// projection clamps to `[0, 1]`; the denominator is floored so a sphere
/// (`a == b`, zero-length segment) stays finite.
fn capsule_sdf(p: Vec3, cap: Capsule) -> f32 {
    let pa = p.subtract(cap.a);
    let ba = cap.b.subtract(cap.a);
    let h = (pa.dot(ba) / ba.dot(ba).max(EPS)).clamp(0.0, 1.0);
    pa.subtract(ba.mul_scalar(h)).length() - cap.radius
}

/// The polynomial smooth-minimum of two distances (blend radius `k`), so unioned
/// capsules fuse with a fillet instead of a hard crease.
fn smin(a: f32, b: f32, k: f32) -> f32 {
    let h = (0.5 + 0.5 * (b - a) / k).clamp(0.0, 1.0);
    let mixed = b * (1.0 - h) + a * h;
    mixed - k * h * (1.0 - h)
}

/// The field value at `p`: the smooth-union (blend radius `k`) of every capsule's
/// signed distance.
fn field(p: Vec3, caps: &[Capsule], k: f32) -> f32 {
    caps.iter().fold(1.0e9, |acc, &cap| smin(acc, capsule_sdf(p, cap), k))
}

/// The outward unit normal at `p` from the field gradient (central differences),
/// falling back to +Y where the gradient vanishes.
fn field_normal(p: Vec3, caps: &[Capsule], k: f32) -> Vec3 {
    let axis = |d: Vec3| field(p.add(d), caps, k) - field(p.subtract(d), caps, k);
    Vec3::new(
        axis(Vec3::new(GRAD_H, 0.0, 0.0)),
        axis(Vec3::new(0.0, GRAD_H, 0.0)),
        axis(Vec3::new(0.0, 0.0, GRAD_H)),
    )
    .normalize()
    .unwrap_or(Vec3::UNIT_Y)
}

/// Parse the params into `(iso, res, k, capsules)`:
/// `[iso, res, k, then 7 per capsule]`. Requires `>= 10` params with
/// `len % 7 == 3` (a 3-word header plus at least one whole capsule); anything
/// else is `None`. The blend radius `k` is floored to `EPS` so the smooth-union
/// division stays finite even if the caller passes `0`.
fn parse(params: &[axiom_recipe::Param]) -> Option<(f32, u32, f32, Vec<Capsule>)> {
    let len = params.len();
    ((len >= 10) & (len % 7 == 3)).then(|| {
        let iso = params[0].as_scalar().get();
        let res = params[1].as_int().clamp(MIN_RES, MAX_RES);
        let k = params[2].as_scalar().get().max(EPS);
        let caps = params[3..]
            .chunks_exact(7)
            .map(|c| Capsule {
                a: Vec3::new(c[0].as_scalar().get(), c[1].as_scalar().get(), c[2].as_scalar().get()),
                b: Vec3::new(c[3].as_scalar().get(), c[4].as_scalar().get(), c[5].as_scalar().get()),
                radius: c[6].as_scalar().get(),
            })
            .collect();
        (iso, res, k, caps)
    })
}

/// The grid's low/high corners: the union of the capsules' inflated AABBs, padded
/// by two cells so the surface sits strictly inside the grid.
fn bounds(caps: &[Capsule], res: u32) -> (Vec3, Vec3) {
    let (lo, hi) = caps.iter().fold((Vec3::new(1.0e30, 1.0e30, 1.0e30), Vec3::new(-1.0e30, -1.0e30, -1.0e30)), |(lo, hi), c| {
        let mn = Vec3::new(c.a.x.min(c.b.x) - c.radius, c.a.y.min(c.b.y) - c.radius, c.a.z.min(c.b.z) - c.radius);
        let mx = Vec3::new(c.a.x.max(c.b.x) + c.radius, c.a.y.max(c.b.y) + c.radius, c.a.z.max(c.b.z) + c.radius);
        (
            Vec3::new(lo.x.min(mn.x), lo.y.min(mn.y), lo.z.min(mn.z)),
            Vec3::new(hi.x.max(mx.x), hi.y.max(mx.y), hi.z.max(mx.z)),
        )
    });
    let span = hi.subtract(lo);
    let pad = span.mul_scalar(2.0 / res as f32);
    (lo.subtract(pad), hi.add(pad))
}

/// Sample the field at every grid corner: a `(res+1)³` scalar buffer.
fn sample_grid(lo: Vec3, cell: Vec3, res: u32, caps: &[Capsule], k: f32) -> Vec<f32> {
    let vx = res + 1;
    (0..vx * vx * vx)
        .map(|i| {
            let p = Vec3::new(
                lo.x + (i % vx) as f32 * cell.x,
                lo.y + ((i / vx) % vx) as f32 * cell.y,
                lo.z + (i / (vx * vx)) as f32 * cell.z,
            );
            field(p, caps, k)
        })
        .collect()
}

/// The triangle-corner positions marching cubes emits for one cell, in order
/// (each run of three is a triangle). An empty result means the cell is wholly
/// inside or outside the surface.
fn cell_vertices(base: [u32; 3], lo: Vec3, cell: Vec3, iso: f32, vx: u32, vals: &[f32]) -> Vec<Vec3> {
    let cval: [f32; 8] = core::array::from_fn(|c| {
        let o = MC_CORNER_OFFSET[c];
        let idx = (((base[2] + o[2]) * vx + (base[1] + o[1])) * vx + (base[0] + o[0])) as usize;
        vals.get(idx).copied().unwrap_or(0.0)
    });
    let cpos: [Vec3; 8] = core::array::from_fn(|c| {
        let o = MC_CORNER_OFFSET[c];
        Vec3::new(
            lo.x + (base[0] + o[0]) as f32 * cell.x,
            lo.y + (base[1] + o[1]) as f32 * cell.y,
            lo.z + (base[2] + o[2]) as f32 * cell.z,
        )
    });
    let cube = (0..8).map(|c| ((cval[c] < iso) as u32) << c).sum::<u32>() as usize;
    MC_TRI_TABLE[cube]
        .iter()
        .take_while(|&&e| e >= 0)
        .map(|&e| {
            let [c0, c1] = MC_EDGE_CORNERS[e as usize];
            let (v0, v1) = (cval[c0], cval[c1]);
            let d = v1 - v0;
            let denom = d + (d.abs() < EPS) as i32 as f32 * EPS;
            let t = ((iso - v0) / denom).clamp(0.0, 1.0);
            cpos[c0].add(cpos[c1].subtract(cpos[c0]).mul_scalar(t))
        })
        .collect()
}

/// **MetaSurface** operator — see [`crate::MeshOp::MetaSurface`].
pub(crate) fn meta_surface(ctx: NodeEval<'_, MeshBuffer>) -> Option<MeshBuffer> {
    parse(ctx.params()).and_then(|(iso, res, k, caps)| {
        let (lo, hi) = bounds(&caps, res);
        let span = hi.subtract(lo);
        let cell = Vec3::new(span.x / res as f32, span.y / res as f32, span.z / res as f32);
        let vx = res + 1;
        let vals = sample_grid(lo, cell, res, &caps, k);
        let positions: Vec<Vec3> = (0..res * res * res)
            .flat_map(|c| cell_vertices([c % res, (c / res) % res, c / (res * res)], lo, cell, iso, vx, &vals))
            .collect();
        let normals = positions.iter().map(|&p| field_normal(p, &caps, k)).collect();
        let uvs = positions.iter().map(|p| Vec2::new(p.x, p.z)).collect();
        let indices = (0..positions.len() as u32).collect();
        MeshBuffer::from_parts(positions, normals, uvs, indices)
    })
}

#[cfg(test)]
mod tests {
    use crate::dispatch::mesh_eval;
    use crate::mesh_buffer::MeshBuffer;
    use crate::mesh_op::MeshOp;
    use axiom_proc_core::ProcCore;
    use axiom_recipe::{Param, RecipeGraph, RecipeId, Scalar};
    use axiom_space::SpaceApi;

    fn s(v: f32) -> Param {
        Param::scalar(Scalar::new(v))
    }

    /// A representative blend radius the tests skin with.
    const K: f32 = 0.15;

    /// Bake a single MetaSurface node from a flat param list.
    fn bake(params: Vec<Param>) -> Option<MeshBuffer> {
        let mut g = RecipeGraph::new(RecipeId::from_raw(1), 1);
        g.add(MeshOp::MetaSurface as u16, params, vec![]);
        ProcCore::new().execute(&g, 7, &SpaceApi::root(), mesh_eval).ok()
    }

    /// `[iso, res, k]` header then one sphere capsule (`a == b`) of the given radius.
    fn sphere(res: u32, cx: f32, radius: f32) -> Vec<Param> {
        vec![s(0.0), Param::int(res), s(K), s(cx), s(0.0), s(0.0), s(cx), s(0.0), s(0.0), s(radius)]
    }

    #[test]
    fn a_single_sphere_bakes_a_closed_surface() {
        let m = bake(sphere(20, 0.0, 1.0)).unwrap();
        assert!(m.triangle_count() > 0, "the iso-surface crosses the grid");
        assert_eq!(m.indices().len(), m.vertex_count(), "unwelded: one index per emitted vertex");
        assert_eq!(m.vertex_count() % 3, 0, "vertices come in whole triangles");
        // Every vertex sits ~1 unit from the centre (the sphere's radius).
        assert!(m.positions().iter().all(|p| (p.length() - 1.0).abs() < 0.2));
    }

    #[test]
    fn every_normal_is_unit_length() {
        let m = bake(sphere(18, 0.0, 1.0)).unwrap();
        assert!(m.normals().iter().all(|n| (n.length() - 1.0).abs() < 1.0e-3));
    }

    #[test]
    fn two_overlapping_spheres_fuse_into_one_connected_surface() {
        // Two spheres a smidge apart, blended by smooth-union: the neck between
        // them is filled, so the surface is a single connected blob (no vertex
        // sits in the gap plane far from both centres).
        let params = vec![
            s(0.0), Param::int(24), s(K), // iso, res, blend radius
            s(-0.6), s(0.0), s(0.0), s(-0.6), s(0.0), s(0.0), s(0.7), // left sphere
            s(0.6), s(0.0), s(0.0), s(0.6), s(0.0), s(0.0), s(0.7), // right sphere
        ];
        let m = bake(params).unwrap();
        assert!(m.triangle_count() > 0);
        // The waist (x≈0) is bridged: some vertices sit near the join plane.
        assert!(m.positions().iter().any(|p| p.x.abs() < 0.1), "the two balls are fused, not disjoint");
    }

    #[test]
    fn a_capsule_is_longer_than_a_sphere() {
        // A vertical capsule (a≠b) spans further in Y than a same-radius sphere.
        let cap = vec![s(0.0), Param::int(20), s(K), s(0.0), s(-0.8), s(0.0), s(0.0), s(0.8), s(0.0), s(0.4)];
        let m = bake(cap).unwrap();
        let y_span = m.positions().iter().map(|p| p.y).fold(f32::MIN, f32::max)
            - m.positions().iter().map(|p| p.y).fold(f32::MAX, f32::min);
        assert!(y_span > 1.2, "capsule spans its segment plus caps, got {y_span}");
    }

    #[test]
    fn baking_is_deterministic() {
        let a = bake(sphere(16, 0.0, 1.0)).unwrap();
        let b = bake(sphere(16, 0.0, 1.0)).unwrap();
        assert_eq!(a, b);
    }

    #[test]
    fn resolution_is_clamped_not_unbounded() {
        // A wildly over-fine request is clamped to MAX_RES — identical to asking
        // for MAX_RES directly (the work is bounded, never honoured unbounded).
        assert_eq!(bake(sphere(9999, 0.0, 0.6)), bake(sphere(super::MAX_RES, 0.0, 0.6)));
        // A shape at a modest res bakes within the vertex cap.
        let m = bake(sphere(30, 0.0, 0.6)).unwrap();
        assert!(m.vertex_count() <= crate::mesh_buffer::MAX_VERTS);
        assert!(m.triangle_count() > 0);
    }

    #[test]
    fn malformed_param_lists_fail() {
        // Header only (no whole capsule).
        assert!(bake(vec![s(0.0), Param::int(8), s(K)]).is_none());
        // A ragged trailing chunk (not a whole 7-word capsule after the header).
        assert!(bake(vec![s(0.0), Param::int(8), s(K), s(0.0), s(0.0), s(0.0)]).is_none());
    }

    #[test]
    fn a_zero_blend_radius_is_floored_and_still_bakes() {
        // The op assumes nothing about the caller's units; a degenerate k=0 is
        // floored to EPS so the smooth-union stays finite (a near-hard union).
        let mut p = sphere(20, 0.0, 1.0);
        p[2] = s(0.0); // k = 0
        let m = bake(p).unwrap();
        assert!(m.triangle_count() > 0);
        assert!(m.positions().iter().all(|v| v.x.is_finite() && v.y.is_finite() && v.z.is_finite()));
    }
}
