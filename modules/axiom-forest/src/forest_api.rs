//! The forest facade: a cell's tree transforms + a unit tree mesh.

use axiom_kernel::{Meters, StableHash};
use axiom_math::{Quat, Transform, Vec3};
use axiom_scatter::{CellCoord, ScatterApi};

use crate::ids::ForestConfig;

/// A chunk's worth of trees.
///
/// Stateless: a cell's trees are a pure function of `(seed, cell, config, ground)`,
/// so the forest tiles identically on every platform and every visit.
#[derive(Debug)]
pub struct ForestApi;

impl ForestApi {
    /// The seated tree transforms of one `cell`. Each scattered site
    /// ([`axiom_scatter`]) becomes a tree seated on the ground at that site (its
    /// height read from `ground`), with a stable per-tree yaw and uniform size
    /// derived from the site's seed. Deterministic and seamless across cells (it
    /// inherits the scatter's tiling).
    pub fn chunk_trees(
        seed: u64,
        cell: CellCoord,
        config: &ForestConfig,
        ground: impl Fn(Meters, Meters) -> Meters,
    ) -> Vec<Transform> {
        let min = config.min_size.get();
        let span = config.max_size.get() - min;
        ScatterApi::chunk_sites(seed, cell, config.cell_size, &config.scatter)
            .iter()
            .map(|site| {
                let h = StableHash::of_words(&[site.seed]).raw();
                let yaw = ((h & 0xFFFF) as f32 / 65_536.0) * std::f32::consts::TAU;
                let size = min + span * (((h >> 16) & 0xFFFF) as f32 / 65_536.0);
                let y = ground(site.x, site.z).get();
                let half = yaw * 0.5;
                let rot = Quat::new(0.0, half.sin(), 0.0, half.cos());
                Transform::new(
                    Vec3::new(site.x.get(), y, site.z.get()),
                    rot,
                    Vec3::new(size, size, size),
                )
            })
            .collect()
    }

    /// A simple unit tree mesh to instance the transforms with: two crossed
    /// vertical quads spanning `y in [0, 1]`, brown at the base fading to canopy
    /// green at the top, double-sided so it reads from any angle. Interleaved
    /// 12-float vertices (`position, normal, uv, colour`) + triangle indices.
    pub fn tree_mesh() -> (Vec<f32>, Vec<u32>) {
        const W: f32 = 0.45;
        let brown = [0.34f32, 0.24, 0.13];
        let green = [0.28f32, 0.44, 0.16];
        let vert = |x: f32, y: f32, z: f32, n: [f32; 3], uv: [f32; 2], c: [f32; 3]| -> [f32; 12] {
            [
                x, y, z, n[0], n[1], n[2], uv[0], uv[1], c[0], c[1], c[2], 1.0,
            ]
        };
        let mut v = Vec::new();
        // Quad in the XY plane (normal +Z): base brown, top green.
        v.extend_from_slice(&vert(-W, 0.0, 0.0, [0.0, 0.0, 1.0], [0.0, 0.0], brown));
        v.extend_from_slice(&vert(W, 0.0, 0.0, [0.0, 0.0, 1.0], [1.0, 0.0], brown));
        v.extend_from_slice(&vert(W, 1.0, 0.0, [0.0, 0.0, 1.0], [1.0, 1.0], green));
        v.extend_from_slice(&vert(-W, 1.0, 0.0, [0.0, 0.0, 1.0], [0.0, 1.0], green));
        // Quad in the ZY plane (normal +X).
        v.extend_from_slice(&vert(0.0, 0.0, -W, [1.0, 0.0, 0.0], [0.0, 0.0], brown));
        v.extend_from_slice(&vert(0.0, 0.0, W, [1.0, 0.0, 0.0], [1.0, 0.0], brown));
        v.extend_from_slice(&vert(0.0, 1.0, W, [1.0, 0.0, 0.0], [1.0, 1.0], green));
        v.extend_from_slice(&vert(0.0, 1.0, -W, [1.0, 0.0, 0.0], [0.0, 1.0], green));
        // Two triangles per quad, both windings → visible from either side.
        let idx = vec![
            0, 1, 2, 0, 2, 3, 0, 2, 1, 0, 3, 2, //
            4, 5, 6, 4, 6, 7, 4, 6, 5, 4, 7, 6,
        ];
        (v, idx)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axiom_kernel::Ratio;
    use axiom_scatter::ScatterRule;

    fn config(cell_size: f32, sites_per_side: u32, min_size: f32, max_size: f32) -> ForestConfig {
        ForestConfig {
            cell_size: Meters::finite_or_zero(cell_size),
            scatter: ScatterRule {
                sites_per_side,
                jitter: Ratio::new(0.7).unwrap(),
                fill: Ratio::new(1.0).unwrap(),
            },
            min_size: Meters::finite_or_zero(min_size),
            max_size: Meters::finite_or_zero(max_size),
        }
    }

    /// Ground plane at a constant height.
    fn flat(h: f32) -> impl Fn(Meters, Meters) -> Meters {
        move |_, _| Meters::finite_or_zero(h)
    }

    #[test]
    fn seats_one_tree_per_scattered_site_on_the_ground() {
        let trees = ForestApi::chunk_trees(
            9,
            CellCoord::new(0, 0),
            &config(16.0, 4, 2.0, 5.0),
            flat(3.0),
        );
        // Full-fill 4×4 sub-grid → 16 trees, each seated at ground height 3.0.
        assert_eq!(trees.len(), 16);
        for t in &trees {
            assert_eq!(t.translation.y, 3.0);
        }
    }

    #[test]
    fn trees_follow_a_sloped_ground_function() {
        // Ground height = x, so each tree's y equals its own x.
        let ground = |x: Meters, _z: Meters| x;
        let trees =
            ForestApi::chunk_trees(1, CellCoord::new(2, 0), &config(10.0, 3, 1.0, 2.0), ground);
        for t in &trees {
            assert_eq!(t.translation.y, t.translation.x);
        }
    }

    #[test]
    fn tree_placement_is_deterministic() {
        let a = ForestApi::chunk_trees(
            4,
            CellCoord::new(-1, 2),
            &config(12.0, 4, 1.0, 3.0),
            flat(0.0),
        );
        let b = ForestApi::chunk_trees(
            4,
            CellCoord::new(-1, 2),
            &config(12.0, 4, 1.0, 3.0),
            flat(0.0),
        );
        assert_eq!(a, b);
        assert!(!a.is_empty());
    }

    #[test]
    fn tree_size_stays_within_the_configured_range() {
        let trees = ForestApi::chunk_trees(
            7,
            CellCoord::new(0, 0),
            &config(16.0, 5, 2.0, 6.0),
            flat(0.0),
        );
        for t in &trees {
            assert!(t.scale.x >= 2.0);
            assert!(t.scale.x <= 6.0);
        }
    }

    #[test]
    fn tree_mesh_is_a_double_sided_crossed_billboard() {
        let (v, idx) = ForestApi::tree_mesh();
        // 8 vertices × 12 floats; 2 quads × 2 tris × 2 windings × 3 = 24 indices.
        assert_eq!(v.len(), 8 * 12);
        assert_eq!(idx.len(), 24);
        // First vertex is brown at the base (colour channels 8..11 of vertex 0).
        assert_eq!([v[8], v[9], v[10]], [0.34, 0.24, 0.13]);
        // Third vertex (top) is canopy green.
        let top = 2 * 12;
        assert_eq!([v[top + 8], v[top + 9], v[top + 10]], [0.28, 0.44, 0.16]);
    }

    #[test]
    fn distinct_seeds_grow_distinct_forests() {
        let a = ForestApi::chunk_trees(
            1,
            CellCoord::new(0, 0),
            &config(16.0, 4, 1.0, 3.0),
            flat(0.0),
        );
        let b = ForestApi::chunk_trees(
            2,
            CellCoord::new(0, 0),
            &config(16.0, 4, 1.0, 3.0),
            flat(0.0),
        );
        assert_ne!(a, b);
    }
}
