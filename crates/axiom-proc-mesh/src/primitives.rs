//! The source mesh operators (no inputs): Cube, Cylinder, Grid.

use core::f32::consts::{PI, TAU};

use axiom_math::{Vec2, Vec3};
use axiom_proc_core::NodeEval;

use crate::mesh_buffer::MeshBuffer;

/// The largest grid subdivision, per axis, that a recipe may request.
pub(crate) const MAX_GRID: u32 = 64;
/// The largest cylinder segment count a recipe may request.
pub(crate) const MAX_SEGMENTS: u32 = 64;

/// The eight cube corners of a unit (`±0.5`) box.
const CORNERS: [[f32; 3]; 8] = [
    [-0.5, -0.5, -0.5],
    [0.5, -0.5, -0.5],
    [0.5, 0.5, -0.5],
    [-0.5, 0.5, -0.5],
    [-0.5, -0.5, 0.5],
    [0.5, -0.5, 0.5],
    [0.5, 0.5, 0.5],
    [-0.5, 0.5, 0.5],
];

/// Each cube face as four CCW-outward corner indices and its outward normal.
const FACES: [([usize; 4], [f32; 3]); 6] = [
    ([4, 5, 6, 7], [0.0, 0.0, 1.0]),
    ([1, 0, 3, 2], [0.0, 0.0, -1.0]),
    ([5, 1, 2, 6], [1.0, 0.0, 0.0]),
    ([0, 4, 7, 3], [-1.0, 0.0, 0.0]),
    ([3, 2, 6, 7], [0.0, 1.0, 0.0]),
    ([4, 5, 1, 0], [0.0, -1.0, 0.0]),
];

/// The four corner UVs of a face quad.
const FACE_UVS: [[f32; 2]; 4] = [[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 1.0]];

/// **Cube** — an axis-aligned box with per-face normals and UVs. Params:
/// `[size]` (full edge length).
pub(crate) fn cube(ctx: NodeEval<'_, MeshBuffer>) -> Option<MeshBuffer> {
    ctx.params()
        .first()
        .map(|p| p.as_scalar().get())
        .and_then(|s| {
            let positions = (0..24)
                .map(|k| {
                    let c = CORNERS[FACES[k / 4].0[k % 4]];
                    Vec3::new(c[0] * s, c[1] * s, c[2] * s)
                })
                .collect();
            let normals = (0..24)
                .map(|k| {
                    let n = FACES[k / 4].1;
                    Vec3::new(n[0], n[1], n[2])
                })
                .collect();
            let uvs = (0..24)
                .map(|k| {
                    let uv = FACE_UVS[k % 4];
                    Vec2::new(uv[0], uv[1])
                })
                .collect();
            let indices = (0..6_u32)
                .flat_map(|f| [0_u32, 1, 2, 0, 2, 3].map(|i| f * 4 + i))
                .collect();
            MeshBuffer::from_parts(positions, normals, uvs, indices)
        })
}

/// **Grid** — a flat `cols`×`rows` plane in the XZ plane, +Y up. Params:
/// `[cols, rows, size]` (full edge length). Subdivision is clamped to
/// [`MAX_GRID`].
pub(crate) fn grid(ctx: NodeEval<'_, MeshBuffer>) -> Option<MeshBuffer> {
    let p = ctx.params();
    (p.len() >= 3).then_some(()).and_then(|()| {
        let cols = p[0].as_int().clamp(1, MAX_GRID);
        let rows = p[1].as_int().clamp(1, MAX_GRID);
        let size = p[2].as_scalar().get();
        let vx = cols + 1;
        let count = vx * (rows + 1);
        let positions = (0..count)
            .map(|k| {
                let x = ((k % vx) as f32 / cols as f32 - 0.5) * size;
                let z = ((k / vx) as f32 / rows as f32 - 0.5) * size;
                Vec3::new(x, 0.0, z)
            })
            .collect();
        let normals = (0..count).map(|_| Vec3::UNIT_Y).collect();
        let uvs = (0..count)
            .map(|k| Vec2::new((k % vx) as f32 / cols as f32, (k / vx) as f32 / rows as f32))
            .collect();
        let indices = (0..cols * rows)
            .flat_map(|q| {
                let a = (q / cols) * vx + (q % cols);
                [a, a + vx, a + 1, a + 1, a + vx, a + vx + 1]
            })
            .collect();
        MeshBuffer::from_parts(positions, normals, uvs, indices)
    })
}

/// **Cylinder** — a capped cylinder about +Y. The cap fans reuse the ring
/// vertices (their normals are radial, so cap shading is approximate — a v0
/// simplification). Params: `[radius, height, segments]`. Segments clamp to
/// `3..=MAX_SEGMENTS`.
pub(crate) fn cylinder(ctx: NodeEval<'_, MeshBuffer>) -> Option<MeshBuffer> {
    let p = ctx.params();
    (p.len() >= 3).then_some(()).and_then(|()| {
        let radius = p[0].as_scalar().get();
        let height = p[1].as_scalar().get();
        let seg = p[2].as_int().clamp(3, MAX_SEGMENTS);
        let angle = |i: u32| TAU * i as f32 / seg as f32;
        // 2*seg ring vertices (bottom then top) plus two cap centers.
        let positions = (0..2 * seg)
            .map(|k| {
                let a = angle(k % seg);
                let y = (k / seg) as f32 * height;
                Vec3::new(radius * a.cos(), y, radius * a.sin())
            })
            .chain([Vec3::new(0.0, 0.0, 0.0), Vec3::new(0.0, height, 0.0)])
            .collect();
        let normals = (0..2 * seg)
            .map(|k| {
                let a = angle(k % seg);
                Vec3::new(a.cos(), 0.0, a.sin())
            })
            .chain([Vec3::new(0.0, -1.0, 0.0), Vec3::new(0.0, 1.0, 0.0)])
            .collect();
        let uvs = (0..2 * seg)
            .map(|k| Vec2::new((k % seg) as f32 / seg as f32, (k / seg) as f32))
            .chain([Vec2::new(0.5, 0.5), Vec2::new(0.5, 0.5)])
            .collect();
        let bottom_center = 2 * seg;
        let top_center = 2 * seg + 1;
        let side = (0..seg).flat_map(move |i| {
            let n = (i + 1) % seg;
            [i, seg + i, n, n, seg + i, seg + n]
        });
        let bottom = (0..seg).flat_map(move |i| [bottom_center, (i + 1) % seg, i]);
        let top = (0..seg).flat_map(move |i| [top_center, seg + i, seg + (i + 1) % seg]);
        let indices = side.chain(bottom).chain(top).collect();
        MeshBuffer::from_parts(positions, normals, uvs, indices)
    })
}

/// **Sphere** — a UV sphere about the origin: `rings` latitude bands by
/// `segments` longitude divisions, with outward unit normals and a lat/long UV
/// wrap. This is the genuinely round primitive the other operators cannot fake
/// (`Bevel` only shrinks a mesh toward its centroid). Params: `[radius, rings,
/// segments]`; `rings` clamps to `2..=MAX_SEGMENTS`, `segments` to
/// `3..=MAX_SEGMENTS`.
pub(crate) fn sphere(ctx: NodeEval<'_, MeshBuffer>) -> Option<MeshBuffer> {
    let p = ctx.params();
    (p.len() >= 3).then_some(()).and_then(|()| {
        let radius = p[0].as_scalar().get();
        let rings = p[1].as_int().clamp(2, MAX_SEGMENTS);
        let seg = p[2].as_int().clamp(3, MAX_SEGMENTS);
        let vx = seg + 1;
        let count = (rings + 1) * vx;
        // Latitude phi (0 = +Y pole .. PI = -Y pole), longitude theta.
        let dir = move |k: u32| {
            let phi = PI * (k / vx) as f32 / rings as f32;
            let theta = TAU * (k % vx) as f32 / seg as f32;
            Vec3::new(phi.sin() * theta.cos(), phi.cos(), phi.sin() * theta.sin())
        };
        let positions = (0..count).map(|k| dir(k).mul_scalar(radius)).collect();
        let normals = (0..count).map(dir).collect();
        let uvs = (0..count)
            .map(|k| Vec2::new((k % vx) as f32 / seg as f32, (k / vx) as f32 / rings as f32))
            .collect();
        let indices = (0..rings * seg)
            .flat_map(move |q| {
                let a = (q / seg) * vx + (q % seg);
                [a, a + vx, a + 1, a + 1, a + vx, a + vx + 1]
            })
            .collect();
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

    fn run(op: MeshOp, params: Vec<Param>) -> Option<MeshBuffer> {
        let mut g = RecipeGraph::new(RecipeId::from_raw(1), 1);
        g.add(op as u16, params, vec![]);
        ProcCore::new()
            .execute(&g, 1, &SpaceApi::root(), mesh_eval)
            .ok()
    }

    #[test]
    fn cube_has_24_vertices_and_12_triangles() {
        let m = run(MeshOp::Cube, vec![Param::scalar(Scalar::new(2.0))]).unwrap();
        assert_eq!(m.vertex_count(), 24);
        assert_eq!(m.triangle_count(), 12);
        // Size 2 → corner at ±1.
        assert!(m.positions().iter().all(|p| p.x.abs() <= 1.0 + 1e-6));
        assert!(run(MeshOp::Cube, vec![]).is_none());
    }

    #[test]
    fn grid_subdivides_and_clamps() {
        let m = run(
            MeshOp::Grid,
            vec![
                Param::int(2),
                Param::int(2),
                Param::scalar(Scalar::new(4.0)),
            ],
        )
        .unwrap();
        assert_eq!(m.vertex_count(), 9); // (2+1)*(2+1)
        assert_eq!(m.triangle_count(), 8); // 2*2 quads * 2
                                           // Oversize subdivision is clamped, not unbounded.
        let big = run(
            MeshOp::Grid,
            vec![
                Param::int(9999),
                Param::int(1),
                Param::scalar(Scalar::new(1.0)),
            ],
        )
        .unwrap();
        assert_eq!(big.vertex_count(), ((super::MAX_GRID + 1) * 2) as usize);
        // Zero subdivision clamps up to 1 (exercises the lower clamp bound).
        let tiny = run(
            MeshOp::Grid,
            vec![
                Param::int(0),
                Param::int(0),
                Param::scalar(Scalar::new(1.0)),
            ],
        )
        .unwrap();
        assert_eq!(tiny.vertex_count(), 4); // (1+1)*(1+1)
        assert!(run(MeshOp::Grid, vec![Param::int(2)]).is_none());
    }

    #[test]
    fn sphere_is_round_bounded_and_clamped() {
        let m = run(
            MeshOp::Sphere,
            vec![
                Param::scalar(Scalar::new(1.0)),
                Param::int(2),
                Param::int(3),
            ],
        )
        .unwrap();
        // (rings+1) * (segments+1) vertices, rings*segments*2 triangles.
        assert_eq!(m.vertex_count(), 3 * 4);
        assert_eq!(m.triangle_count(), 2 * 3 * 2);
        // Every vertex sits on the sphere of the requested radius (genuine
        // curvature — this is what a beveled cube cannot produce).
        assert!(m
            .positions()
            .iter()
            .all(|p| ((p.x * p.x + p.y * p.y + p.z * p.z).sqrt() - 1.0).abs() < 1e-5));
        // Rings/segments clamp at both ends.
        let coarse = run(
            MeshOp::Sphere,
            vec![
                Param::scalar(Scalar::new(1.0)),
                Param::int(0),
                Param::int(1),
            ],
        )
        .unwrap();
        assert_eq!(coarse.vertex_count(), (2 + 1) * (3 + 1)); // rings→2, seg→3
        let fine = run(
            MeshOp::Sphere,
            vec![
                Param::scalar(Scalar::new(1.0)),
                Param::int(9999),
                Param::int(9999),
            ],
        )
        .unwrap();
        assert_eq!(
            fine.vertex_count(),
            ((super::MAX_SEGMENTS + 1) * (super::MAX_SEGMENTS + 1)) as usize
        );
        assert!(run(MeshOp::Sphere, vec![Param::scalar(Scalar::new(1.0))]).is_none());
    }

    #[test]
    fn cylinder_is_closed_and_bounded() {
        let m = run(
            MeshOp::Cylinder,
            vec![
                Param::scalar(Scalar::new(1.0)),
                Param::scalar(Scalar::new(2.0)),
                Param::int(8),
            ],
        )
        .unwrap();
        assert_eq!(m.vertex_count(), 2 * 8 + 2);
        assert_eq!(
            m.triangle_count(),
            8 /*side*/ * 2 + 8 /*bottom*/ + 8 /*top*/
        );
        // Segment count clamps at both ends (below 3 → 3, above the cap → cap).
        let coarse = run(
            MeshOp::Cylinder,
            vec![
                Param::scalar(Scalar::new(1.0)),
                Param::scalar(Scalar::new(1.0)),
                Param::int(1),
            ],
        )
        .unwrap();
        assert_eq!(coarse.vertex_count(), 2 * 3 + 2);
        let fine = run(
            MeshOp::Cylinder,
            vec![
                Param::scalar(Scalar::new(1.0)),
                Param::scalar(Scalar::new(1.0)),
                Param::int(9999),
            ],
        )
        .unwrap();
        assert_eq!(fine.vertex_count(), (2 * super::MAX_SEGMENTS + 2) as usize);
        assert!(run(MeshOp::Cylinder, vec![Param::scalar(Scalar::new(1.0))]).is_none());
    }
}
