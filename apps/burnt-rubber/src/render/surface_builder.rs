//! Building triangle geometry with **guaranteed-correct winding**.
//!
//! Every procedural mesh in this app is assembled from quads laid along the
//! road's local frame, and getting a quad's corner order wrong is the single
//! easiest mistake to make in that kind of code — the surface is then invisible
//! from above (back-face culled), or, on the Canvas2D backend, flat-shaded from
//! the winding and lit from *underneath*.
//!
//! So the builder never trusts the caller's corner order. [`SurfaceBuilder::quad`]
//! takes the four corners **and the direction the face should point**, checks the
//! resulting triangle normal against it, and reverses the order when they
//! disagree. The engine's own convention — `(b − a) × (c − a)` points out of the
//! face, matching the builtin plane — is encoded here once, and every road
//! surface, marking, guardrail and prop in the app inherits it.

use axiom::prelude::{MeshData, Vec2, Vec3};

/// An accumulating triangle mesh with outward-facing winding by construction.
#[derive(Debug, Default, Clone)]
pub struct SurfaceBuilder {
    positions: Vec<Vec3>,
    normals: Vec<Vec3>,
    uvs: Vec<Vec2>,
    indices: Vec<u32>,
}

impl SurfaceBuilder {
    /// An empty builder.
    pub fn new() -> SurfaceBuilder {
        SurfaceBuilder::default()
    }

    /// An empty builder with room for `quads` quads.
    pub fn with_quad_capacity(quads: usize) -> SurfaceBuilder {
        SurfaceBuilder {
            positions: Vec::with_capacity(quads * 4),
            normals: Vec::with_capacity(quads * 4),
            uvs: Vec::with_capacity(quads * 4),
            indices: Vec::with_capacity(quads * 6),
        }
    }

    /// How many triangles have been emitted.
    pub fn triangle_count(&self) -> usize {
        self.indices.len() / 3
    }

    /// Whether anything has been emitted.
    pub fn is_empty(&self) -> bool {
        self.indices.is_empty()
    }

    /// Add a quad `a → b → c → d` whose face should point along `facing`.
    ///
    /// The corner *cycle* is respected; only its direction may be reversed. A
    /// degenerate quad (zero-area, or one whose normal is perpendicular to
    /// `facing`) is still emitted in the given order — it contributes nothing
    /// visible, and silently dropping geometry would be worse than drawing a
    /// sliver.
    ///
    /// The quad's texture is stretched **once** across it. That is the right
    /// default for a prop the size of a post or a bumper; it is the wrong one for
    /// a surface whose size is not the texture's — see [`Self::quad_with_uvs`].
    pub fn quad(&mut self, a: Vec3, b: Vec3, c: Vec3, d: Vec3, facing: Vec3) {
        self.quad_with_uvs(a, b, c, d, facing, UNIT_UVS);
    }

    /// Add a quad with the texture coordinate of each corner given explicitly.
    ///
    /// The UVs travel **with their corners** through the winding correction, so a
    /// quad whose cycle has to be reversed still samples the same texel at the
    /// same point in space. Corner UVs outside `0..=1` are the point: the material
    /// sampler wraps with `Repeat`, so a caller that derives them from world
    /// metres gets a texture tiled at a real physical scale instead of stretched
    /// once across whatever the quad happens to span.
    pub fn quad_with_uvs(
        &mut self,
        a: Vec3,
        b: Vec3,
        c: Vec3,
        d: Vec3,
        facing: Vec3,
        uvs: [Vec2; 4],
    ) {
        let base = self.positions.len() as u32;
        let computed = b.subtract(a).cross(c.subtract(a));
        let forward = computed.dot(facing) >= 0.0;
        let cycle = [(a, uvs[0]), (b, uvs[1]), (c, uvs[2]), (d, uvs[3])];
        let ordered = if forward {
            cycle
        } else {
            [cycle[0], cycle[3], cycle[2], cycle[1]]
        };
        let normal = facing.normalize().unwrap_or(Vec3::UNIT_Y);
        for (corner, uv) in ordered {
            self.positions.push(corner);
            self.normals.push(normal);
            self.uvs.push(uv);
        }
        self.indices
            .extend_from_slice(&[base, base + 1, base + 2, base, base + 2, base + 3]);
    }

    /// Add a horizontal quad facing up. The overwhelmingly common case: road
    /// surface, markings, verges.
    pub fn ground_quad(&mut self, a: Vec3, b: Vec3, c: Vec3, d: Vec3) {
        self.ground_quad_with_uvs(a, b, c, d, UNIT_UVS);
    }

    /// A horizontal quad facing up, with explicit corner UVs — the paved surface,
    /// whose grain is tiled in world metres rather than smeared once across an
    /// 18 m × 2 m panel.
    pub fn ground_quad_with_uvs(&mut self, a: Vec3, b: Vec3, c: Vec3, d: Vec3, uvs: [Vec2; 4]) {
        // The face's own plane decides which way "up" is, so a banked or graded
        // road panel still faces out of itself rather than out of world +Y.
        let plane = b.subtract(a).cross(d.subtract(a));
        let up = if plane.y >= 0.0 { plane } else { plane.mul_scalar(-1.0) };
        self.quad_with_uvs(a, b, c, d, unit_up(up), uvs);
    }

    /// Add an axis-aligned box spanning `centre ± half`, all six faces outward.
    pub fn box_at(&mut self, centre: Vec3, half: Vec3) {
        self.oriented_box(centre, Vec3::UNIT_X, Vec3::UNIT_Y, Vec3::UNIT_Z, half);
    }

    /// Add a box centred at `centre`, oriented by the (assumed orthonormal)
    /// basis `right`/`up`/`forward`, with the given half extents along each.
    pub fn oriented_box(
        &mut self,
        centre: Vec3,
        right: Vec3,
        up: Vec3,
        forward: Vec3,
        half: Vec3,
    ) {
        let r = right.mul_scalar(half.x);
        let u = up.mul_scalar(half.y);
        let f = forward.mul_scalar(half.z);
        let corner = |sr: f32, su: f32, sf: f32| {
            centre
                .add(r.mul_scalar(sr))
                .add(u.mul_scalar(su))
                .add(f.mul_scalar(sf))
        };
        // Each face: four corners in a consistent cycle, plus its outward axis.
        // The builder fixes the direction, so only the cycle has to be right.
        let faces: [([(f32, f32, f32); 4], Vec3); 6] = [
            (
                [(-1.0, -1.0, 1.0), (1.0, -1.0, 1.0), (1.0, 1.0, 1.0), (-1.0, 1.0, 1.0)],
                forward,
            ),
            (
                [(-1.0, -1.0, -1.0), (1.0, -1.0, -1.0), (1.0, 1.0, -1.0), (-1.0, 1.0, -1.0)],
                forward.mul_scalar(-1.0),
            ),
            (
                [(1.0, -1.0, -1.0), (1.0, -1.0, 1.0), (1.0, 1.0, 1.0), (1.0, 1.0, -1.0)],
                right,
            ),
            (
                [(-1.0, -1.0, -1.0), (-1.0, -1.0, 1.0), (-1.0, 1.0, 1.0), (-1.0, 1.0, -1.0)],
                right.mul_scalar(-1.0),
            ),
            (
                [(-1.0, 1.0, -1.0), (1.0, 1.0, -1.0), (1.0, 1.0, 1.0), (-1.0, 1.0, 1.0)],
                up,
            ),
            (
                [(-1.0, -1.0, -1.0), (1.0, -1.0, -1.0), (1.0, -1.0, 1.0), (-1.0, -1.0, 1.0)],
                up.mul_scalar(-1.0),
            ),
        ];
        for (cycle, facing) in faces {
            self.quad(
                corner(cycle[0].0, cycle[0].1, cycle[0].2),
                corner(cycle[1].0, cycle[1].1, cycle[1].2),
                corner(cycle[2].0, cycle[2].1, cycle[2].2),
                corner(cycle[3].0, cycle[3].1, cycle[3].2),
                facing,
            );
        }
    }

    /// Add a `sides`-sided cone from `base` up `axis`, of radius `radius`. Used
    /// for tree crowns, which need a silhouette the primitive set cannot make.
    pub fn cone(&mut self, base: Vec3, axis: Vec3, radius: f32, sides: u32) {
        let up = unit_up(axis);
        let apex = base.add(axis);
        let (right, forward) = perpendicular_basis(up);
        let sides = sides.max(3);
        let ring = |i: u32| {
            let a = i as f32 / sides as f32 * std::f32::consts::TAU;
            base.add(right.mul_scalar(a.cos() * radius))
                .add(forward.mul_scalar(a.sin() * radius))
        };
        for i in 0..sides {
            let p = ring(i);
            let q = ring((i + 1) % sides);
            // A cone side as a degenerate quad (apex twice) keeps the whole
            // builder to one primitive.
            let outward = p.add(q).mul_scalar(0.5).subtract(base);
            self.quad(p, q, apex, apex, outward.add(up.mul_scalar(radius * 0.5)));
            // The base disc, so the silhouette is closed from below.
            self.quad(base, q, p, p, up.mul_scalar(-1.0));
        }
    }

    /// Finish into engine mesh data.
    pub fn build(self) -> MeshData {
        MeshData::new(self.positions, self.normals, self.uvs, self.indices)
    }

    /// The accumulated positions (for tests and diagnostics).
    pub fn positions(&self) -> &[Vec3] {
        &self.positions
    }

    /// The accumulated indices (for tests and diagnostics).
    pub fn indices(&self) -> &[u32] {
        &self.indices
    }
}

/// The corner UVs that stretch a texture exactly once across a quad.
const UNIT_UVS: [Vec2; 4] = [
    Vec2::new(0.0, 0.0),
    Vec2::new(1.0, 0.0),
    Vec2::new(1.0, 1.0),
    Vec2::new(0.0, 1.0),
];

/// A unit vector pointing generally up, falling back to world up.
fn unit_up(v: Vec3) -> Vec3 {
    v.normalize().unwrap_or(Vec3::UNIT_Y)
}

/// Two unit vectors perpendicular to `axis` and to each other.
fn perpendicular_basis(axis: Vec3) -> (Vec3, Vec3) {
    let seed = if axis.y.abs() > 0.9 {
        Vec3::UNIT_X
    } else {
        Vec3::UNIT_Y
    };
    let right = axis.cross(seed).normalize().unwrap_or(Vec3::UNIT_X);
    let forward = axis.cross(right).normalize().unwrap_or(Vec3::UNIT_Z);
    (right, forward)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The engine's convention, restated as an assertion: for triangle
    /// `(a, b, c)`, `(b − a) × (c − a)` points out of the face.
    fn triangle_normals(builder: &SurfaceBuilder) -> Vec<Vec3> {
        builder
            .indices()
            .chunks(3)
            .map(|t| {
                let a = builder.positions()[t[0] as usize];
                let b = builder.positions()[t[1] as usize];
                let c = builder.positions()[t[2] as usize];
                b.subtract(a).cross(c.subtract(a))
            })
            .collect()
    }

    #[test]
    fn a_quad_faces_the_way_it_was_asked_to() {
        let corners = [
            Vec3::new(-1.0, 0.0, -1.0),
            Vec3::new(1.0, 0.0, -1.0),
            Vec3::new(1.0, 0.0, 1.0),
            Vec3::new(-1.0, 0.0, 1.0),
        ];
        let mut up = SurfaceBuilder::new();
        up.quad(corners[0], corners[1], corners[2], corners[3], Vec3::UNIT_Y);
        for n in triangle_normals(&up) {
            assert!(n.y > 0.0, "asked to face up, got {n:?}");
        }

        // The *same* corner order asked to face down comes out reversed.
        let mut down = SurfaceBuilder::new();
        down.quad(
            corners[0],
            corners[1],
            corners[2],
            corners[3],
            Vec3::new(0.0, -1.0, 0.0),
        );
        for n in triangle_normals(&down) {
            assert!(n.y < 0.0, "asked to face down, got {n:?}");
        }
    }

    #[test]
    fn a_reversed_corner_order_is_corrected_rather_than_trusted() {
        let a = Vec3::new(-1.0, 0.0, -1.0);
        let b = Vec3::new(1.0, 0.0, -1.0);
        let c = Vec3::new(1.0, 0.0, 1.0);
        let d = Vec3::new(-1.0, 0.0, 1.0);
        let mut forward = SurfaceBuilder::new();
        forward.quad(a, b, c, d, Vec3::UNIT_Y);
        let mut backward = SurfaceBuilder::new();
        backward.quad(d, c, b, a, Vec3::UNIT_Y);
        // Both face up despite opposite input cycles.
        for n in triangle_normals(&forward).into_iter().chain(triangle_normals(&backward)) {
            assert!(n.y > 0.0);
        }
    }

    /// A corner's UV belongs to that corner, not to the slot it lands in after
    /// the winding correction. If it did not, a reversed quad would sample a
    /// mirrored patch of texture and the world-metre road mapping would fold back
    /// on itself at every quad the builder had to flip.
    #[test]
    fn uvs_travel_with_their_corners_through_the_winding_correction() {
        let a = Vec3::new(-1.0, 0.0, -1.0);
        let b = Vec3::new(1.0, 0.0, -1.0);
        let c = Vec3::new(1.0, 0.0, 1.0);
        let d = Vec3::new(-1.0, 0.0, 1.0);
        let uvs = [
            Vec2::new(4.0, 7.0),
            Vec2::new(6.0, 7.0),
            Vec2::new(6.0, 9.0),
            Vec2::new(4.0, 9.0),
        ];
        // The same four (corner, uv) pairs, wound both ways.
        let pair_of = |builder: &SurfaceBuilder| {
            let mut pairs: Vec<(String, String)> = builder
                .positions()
                .iter()
                .zip(&builder.uvs)
                .map(|(p, uv)| (format!("{p:?}"), format!("{uv:?}")))
                .collect();
            pairs.sort();
            pairs
        };
        let mut forward = SurfaceBuilder::new();
        forward.quad_with_uvs(a, b, c, d, Vec3::UNIT_Y, uvs);
        let mut reversed = SurfaceBuilder::new();
        reversed.quad_with_uvs(a, b, c, d, Vec3::new(0.0, -1.0, 0.0), uvs);
        assert_eq!(
            pair_of(&forward),
            pair_of(&reversed),
            "reversing the winding re-assigned the texture coordinates"
        );
        // And out-of-range UVs survive untouched — `Repeat` addressing is the
        // whole mechanism behind tiling in world metres.
        assert!(forward.uvs.iter().any(|uv| uv.x > 1.0 && uv.y > 1.0));
    }

    /// The default is still one tile stretched across the quad, for every prop
    /// that has no world-scale mapping of its own.
    #[test]
    fn a_plain_quad_still_stretches_its_texture_once_across_itself() {
        let mut b = SurfaceBuilder::new();
        b.ground_quad(
            Vec3::new(0.0, 0.0, 0.0),
            Vec3::new(9.0, 0.0, 0.0),
            Vec3::new(9.0, 0.0, 9.0),
            Vec3::new(0.0, 0.0, 9.0),
        );
        for uv in &b.uvs {
            assert!((0.0..=1.0).contains(&uv.x) && (0.0..=1.0).contains(&uv.y), "{uv:?}");
        }
    }

    #[test]
    fn stored_normals_match_the_requested_facing() {
        let mut b = SurfaceBuilder::new();
        b.quad(
            Vec3::new(0.0, 0.0, 0.0),
            Vec3::new(1.0, 0.0, 0.0),
            Vec3::new(1.0, 0.0, 1.0),
            Vec3::new(0.0, 0.0, 1.0),
            Vec3::new(0.0, 4.0, 0.0),
        );
        let data = b.build();
        for n in data.normals() {
            assert!((n.length() - 1.0).abs() < 1.0e-5, "normals are unit");
            assert!(n.y > 0.99);
        }
        assert_eq!(data.positions().len(), 4);
        assert_eq!(data.uvs().len(), 4);
        assert_eq!(data.indices().len(), 6);
    }

    #[test]
    fn a_ground_quad_faces_up_whichever_way_it_is_wound() {
        let a = Vec3::new(0.0, 0.0, 0.0);
        let b = Vec3::new(4.0, 0.3, 0.0);
        let c = Vec3::new(4.0, 0.3, 6.0);
        let d = Vec3::new(0.0, 0.0, 6.0);
        for corners in [[a, b, c, d], [d, c, b, a]] {
            let mut builder = SurfaceBuilder::new();
            builder.ground_quad(corners[0], corners[1], corners[2], corners[3]);
            for n in triangle_normals(&builder) {
                assert!(n.y > 0.0, "a banked road panel still faces up: {n:?}");
            }
        }
    }

    #[test]
    fn every_face_of_a_box_points_outward() {
        let mut b = SurfaceBuilder::new();
        let centre = Vec3::new(3.0, 2.0, -1.0);
        b.box_at(centre, Vec3::new(1.0, 2.0, 0.5));
        assert_eq!(b.triangle_count(), 12, "six quads");
        for (i, t) in b.indices().chunks(3).enumerate() {
            let a = b.positions()[t[0] as usize];
            let p = b.positions()[t[1] as usize];
            let q = b.positions()[t[2] as usize];
            let normal = p.subtract(a).cross(q.subtract(a));
            let outward = a.add(p).add(q).mul_scalar(1.0 / 3.0).subtract(centre);
            assert!(
                normal.dot(outward) > 0.0,
                "triangle {i} faces inward: normal {normal:?} vs outward {outward:?}"
            );
        }
    }

    #[test]
    fn an_oriented_box_is_the_axis_aligned_one_when_the_basis_is_identity() {
        let mut plain = SurfaceBuilder::new();
        plain.box_at(Vec3::ZERO, Vec3::ONE);
        let mut oriented = SurfaceBuilder::new();
        oriented.oriented_box(Vec3::ZERO, Vec3::UNIT_X, Vec3::UNIT_Y, Vec3::UNIT_Z, Vec3::ONE);
        assert_eq!(plain.positions(), oriented.positions());
        assert_eq!(plain.indices(), oriented.indices());
    }

    #[test]
    fn an_oriented_box_follows_its_basis() {
        let mut b = SurfaceBuilder::new();
        // Rotated 90 degrees about Y: "forward" is +X.
        b.oriented_box(
            Vec3::ZERO,
            Vec3::new(0.0, 0.0, -1.0),
            Vec3::UNIT_Y,
            Vec3::UNIT_X,
            Vec3::new(1.0, 1.0, 5.0),
        );
        let furthest = b
            .positions()
            .iter()
            .map(|p| p.x.abs())
            .fold(0.0f32, f32::max);
        assert!((furthest - 5.0).abs() < 1.0e-4, "the long axis is now X");
    }

    #[test]
    fn a_cone_is_closed_and_faces_outward_at_the_base() {
        let mut b = SurfaceBuilder::new();
        b.cone(Vec3::ZERO, Vec3::new(0.0, 4.0, 0.0), 1.5, 8);
        assert!(b.triangle_count() > 8);
        let data = b.build();
        assert!(data.positions().iter().all(|p| p.y >= -1.0e-5 && p.y <= 4.0 + 1.0e-5));
        assert!(data
            .positions()
            .iter()
            .all(|p| p.x.is_finite() && p.y.is_finite() && p.z.is_finite()));
        // A cone with too few sides is promoted to a triangle rather than
        // producing nothing.
        let mut degenerate = SurfaceBuilder::new();
        degenerate.cone(Vec3::ZERO, Vec3::UNIT_Y, 1.0, 0);
        assert!(!degenerate.is_empty());
    }

    #[test]
    fn a_degenerate_quad_is_emitted_rather_than_dropped() {
        let mut b = SurfaceBuilder::new();
        let p = Vec3::new(1.0, 1.0, 1.0);
        b.quad(p, p, p, p, Vec3::UNIT_Y);
        assert_eq!(b.triangle_count(), 2);
        // And the fallback normal is still a unit vector.
        let data = b.build();
        assert!(data.normals().iter().all(|n| (n.length() - 1.0).abs() < 1.0e-5));
    }

    #[test]
    fn capacity_hints_do_not_change_the_result() {
        let build = |mut b: SurfaceBuilder| {
            b.box_at(Vec3::ZERO, Vec3::ONE);
            b
        };
        let a = build(SurfaceBuilder::new());
        let c = build(SurfaceBuilder::with_quad_capacity(6));
        assert_eq!(a.positions(), c.positions());
        assert_eq!(a.indices(), c.indices());
        assert!(SurfaceBuilder::new().is_empty());
    }

    #[test]
    fn a_perpendicular_basis_is_orthonormal_for_any_axis() {
        for axis in [
            Vec3::UNIT_Y,
            Vec3::new(0.0, -1.0, 0.0),
            Vec3::new(1.0, 0.0, 0.0),
            Vec3::new(0.3, 0.9, -0.2).normalize().unwrap(),
        ] {
            let (r, f) = perpendicular_basis(axis);
            assert!((r.length() - 1.0).abs() < 1.0e-4);
            assert!((f.length() - 1.0).abs() < 1.0e-4);
            assert!(r.dot(f).abs() < 1.0e-4);
            assert!(r.dot(axis).abs() < 1.0e-4);
        }
    }
}
