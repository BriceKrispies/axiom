//! Deterministic expansion of a manifest `[scatter]` block into explicit tree
//! instances. This is **not** procedural world generation: it is a pure, seeded
//! function of the authored `[scatter]` inputs, producing the same forest on every
//! platform. It exists only so a diorama can carry a hundred trees without the file
//! listing each one by hand — the expanded instances are the same data an author
//! could have typed.
//!
//! Placement rule: draw `count` candidate sites uniformly over the terrain patch,
//! reject any within `min_spacing_m` of an already-placed tree or on ground steeper
//! than `slope_limit`, and seat each accepted tree on the terrain surface.

use axiom_entropy::{EntropyApi, EntropyStream};
use axiom_space::{Address, SpaceApi};

use crate::growth::curves::lerp;

use super::scene::{Groundcover, Scatter, Terrain, Tree, Tuft};

/// Fixed address segment keying the scatter entropy stream ("vtscat\0\x01"), so the
/// stream derived from `(scatter.seed, address, version)` is reproducible.
const SCATTER_SEGMENT: u64 = 0x_76_74_73_63_61_74_00_01;
const SCATTER_VERSION: u32 = 1;
/// Fixed address segment keying the ground-cover entropy stream ("vtgcvr\0\x01"),
/// distinct from the tree scatter so the two streams never correlate.
const GROUNDCOVER_SEGMENT: u64 = 0x_76_74_67_63_76_72_00_01;
/// How many rejected candidates to tolerate before giving up on reaching `count`
/// (keeps a dense/steep scene from looping forever). A generous multiple of `count`.
const MAX_ATTEMPTS_PER_TREE: u32 = 32;

/// Expand `scatter` over `terrain` into concrete [`Tree`] instances, keeping a
/// `clear_center` (the camera's ground `[x, z]`) free of trunks within
/// `scatter.keep_clear_m` so a dense forest never blocks the frame from the camera.
pub fn expand(scatter: &Scatter, terrain: &Terrain, clear_center: [f32; 2]) -> Vec<Tree> {
    let address: Address = SpaceApi::child(&SpaceApi::root(), SCATTER_SEGMENT);
    let mut stream = EntropyApi::stream(scatter.seed, &address, SCATTER_VERSION);

    let half = terrain.half_m();
    let min_sq = scatter.min_spacing_m * scatter.min_spacing_m;
    let clear_sq = scatter.keep_clear_m * scatter.keep_clear_m;
    // Clump centres, drawn up front so placement can gather around them (clearings
    // fall between). Empty when `clusters == 0` → uniform placement.
    let centers: Vec<(f32, f32)> = (0..scatter.clusters)
        .map(|_| (lerp(-half, half, unit(&mut stream)), lerp(-half, half, unit(&mut stream))))
        .collect();
    let mut placed: Vec<Tree> = Vec::with_capacity(scatter.count as usize);
    let attempt_cap = scatter.count.saturating_mul(MAX_ATTEMPTS_PER_TREE);
    let mut attempts = 0u32;

    while (placed.len() as u32) < scatter.count && attempts < attempt_cap {
        attempts += 1;
        let (x, z) = site(&mut stream, &centers, half, scatter.cluster_radius_m);

        // Reject sites inside the camera's keep-clear radius (no face-blocking trunk).
        let cdx = x - clear_center[0];
        let cdz = z - clear_center[1];
        if cdx * cdx + cdz * cdz < clear_sq {
            continue;
        }
        // Reject steep ground.
        if terrain.slope_at(x, z) > scatter.slope_limit {
            continue;
        }
        // Reject sites too close to an existing tree.
        let crowded = placed
            .iter()
            .any(|t| (t.x - x) * (t.x - x) + (t.z - z) * (t.z - z) < min_sq);
        if crowded {
            continue;
        }

        let trunk_height_m = range(&mut stream, scatter.trunk_height_m);
        let trunk_radius_m = range(&mut stream, scatter.trunk_radius_m);
        let canopy_radius_m = range(&mut stream, scatter.canopy_radius_m);
        let yaw_deg = range(&mut stream, [0.0, 360.0]);
        let canopy_color = scatter.canopy_palette[stream.pick_index(scatter.canopy_palette.len())];

        placed.push(Tree {
            x,
            z,
            yaw_deg,
            trunk_height_m,
            trunk_radius_m,
            canopy_radius_m,
            canopy_color,
        });
    }

    placed
}

/// Expand `groundcover` over `terrain` into concrete [`Tuft`] instances — the
/// ground-level analogue of [`expand`], on its own entropy stream.
pub fn expand_groundcover(gc: &Groundcover, terrain: &Terrain) -> Vec<Tuft> {
    let address: Address = SpaceApi::child(&SpaceApi::root(), GROUNDCOVER_SEGMENT);
    let mut stream = EntropyApi::stream(gc.seed, &address, SCATTER_VERSION);

    let half = terrain.half_m();
    let min_sq = gc.min_spacing_m * gc.min_spacing_m;
    // Clump centres (empty → uniform), so ground clutter gathers into patches.
    let centers: Vec<(f32, f32)> = (0..gc.clusters)
        .map(|_| (lerp(-half, half, unit(&mut stream)), lerp(-half, half, unit(&mut stream))))
        .collect();
    let mut placed: Vec<Tuft> = Vec::with_capacity(gc.count as usize);
    let attempt_cap = gc.count.saturating_mul(MAX_ATTEMPTS_PER_TREE);
    let mut attempts = 0u32;

    while (placed.len() as u32) < gc.count && attempts < attempt_cap {
        attempts += 1;
        let (x, z) = site(&mut stream, &centers, half, gc.cluster_radius_m);

        if terrain.slope_at(x, z) > gc.slope_limit {
            continue;
        }
        let crowded = placed
            .iter()
            .any(|t| (t.x - x) * (t.x - x) + (t.z - z) * (t.z - z) < min_sq);
        if crowded {
            continue;
        }

        let height_m = range(&mut stream, gc.height_m);
        let radius_m = range(&mut stream, gc.radius_m);
        let yaw_deg = range(&mut stream, [0.0, 360.0]);
        let color = gc.palette[stream.pick_index(gc.palette.len())];
        placed.push(Tuft { x, z, yaw_deg, height_m, radius_m, color });
    }
    placed
}

/// A candidate placement site. With no clump centres it is uniform over the patch
/// (unchanged sequence); with centres it gathers around a randomly-chosen centre
/// (`sqrt` radius → uniform disc density), so trees clump and clearings open between.
fn site(stream: &mut EntropyStream, centers: &[(f32, f32)], half: f32, cluster_radius_m: f32) -> (f32, f32) {
    if centers.is_empty() {
        return (lerp(-half, half, unit(stream)), lerp(-half, half, unit(stream)));
    }
    let (cx, cz) = centers[stream.pick_index(centers.len())];
    let ang = unit(stream) * std::f32::consts::TAU;
    let r = unit(stream).sqrt() * cluster_radius_m;
    (cx + r * ang.cos(), cz + r * ang.sin())
}

/// A uniform `[0, 1)` sample as `f32`, off the deterministic stream.
fn unit(stream: &mut EntropyStream) -> f32 {
    stream.unit().get()
}

/// A uniform sample in `[lo, hi]` from a `[min, max]` range.
fn range(stream: &mut EntropyStream, bounds: [f32; 2]) -> f32 {
    lerp(bounds[0], bounds[1], unit(stream))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::growth::visual_target::scene::{Groundcover, Octave};

    fn terrain(slope: [f32; 2]) -> Terrain {
        Terrain {
            size_m: 64.0,
            spacing_m: 1.0,
            base_height_m: 0.0,
            slope,
            detail: vec![Octave { amplitude_m: 0.5, wavelength_m: 10.0, seed: 1 }],
            ground_bands: vec![],
            rock_albedo: [0.4, 0.4, 0.4],
            rock_slope_start: 0.45,
            rock_slope_full: 0.95,
        }
    }

    fn scatter(count: u32, slope_limit: f32) -> Scatter {
        Scatter {
            seed: 42,
            count,
            min_spacing_m: 1.5,
            slope_limit,
            keep_clear_m: 0.0,
            clusters: 0,
            cluster_radius_m: 0.0,
            lean_deg: 0.0,
            trunk_height_m: [4.0, 8.0],
            trunk_radius_m: [0.2, 0.4],
            canopy_radius_m: [2.0, 4.0],
            canopy_palette: vec![[0.8, 0.4, 0.1], [0.86, 0.62, 0.18]],
        }
    }

    #[test]
    fn same_seed_reproduces_the_forest() {
        let t = terrain([0.02, 0.02]);
        let a = expand(&scatter(60, 1.0), &t, [0.0, 0.0]);
        let b = expand(&scatter(60, 1.0), &t, [0.0, 0.0]);
        assert_eq!(a.len(), b.len());
        for (ta, tb) in a.iter().zip(&b) {
            assert_eq!(ta.x, tb.x);
            assert_eq!(ta.z, tb.z);
            assert_eq!(ta.trunk_height_m, tb.trunk_height_m);
            assert_eq!(ta.canopy_color, tb.canopy_color);
        }
    }

    #[test]
    fn respects_minimum_spacing() {
        let t = terrain([0.0, 0.0]);
        let s = scatter(80, 1.0);
        let trees = expand(&s, &t, [0.0, 0.0]);
        let min_sq = s.min_spacing_m * s.min_spacing_m;
        for (i, a) in trees.iter().enumerate() {
            for b in &trees[i + 1..] {
                let d = (a.x - b.x).powi(2) + (a.z - b.z).powi(2);
                assert!(d >= min_sq, "two trees closer than min spacing");
            }
        }
    }

    fn groundcover(count: u32) -> Groundcover {
        Groundcover {
            seed: 5,
            count,
            min_spacing_m: 0.5,
            slope_limit: 1.0,
            height_m: [0.2, 0.6],
            radius_m: [0.1, 0.4],
            palette: vec![[0.6, 0.5, 0.2], [0.7, 0.4, 0.2]],
            clusters: 0,
            cluster_radius_m: 0.0,
        }
    }

    #[test]
    fn groundcover_is_deterministic_and_respects_spacing() {
        let t = terrain([0.02, 0.02]);
        let gc = groundcover(200);
        let a = expand_groundcover(&gc, &t);
        let b = expand_groundcover(&gc, &t);
        assert_eq!(a.len(), b.len());
        assert!(!a.is_empty());
        for (p, q) in a.iter().zip(&b) {
            assert_eq!(p.x, q.x);
            assert_eq!(p.z, q.z);
            assert_eq!(p.color, q.color);
            assert_eq!(p.height_m, q.height_m);
        }
        let min_sq = gc.min_spacing_m * gc.min_spacing_m;
        for (i, p) in a.iter().enumerate() {
            for q in &a[i + 1..] {
                assert!((p.x - q.x).powi(2) + (p.z - q.z).powi(2) >= min_sq);
            }
        }
    }

    #[test]
    fn steep_slope_limit_places_fewer_trees() {
        // A steep linear ramp with a strict slope limit rejects most sites.
        let steep = terrain([0.9, 0.9]);
        let strict = expand(&scatter(80, 0.2), &steep, [0.0, 0.0]);
        let permissive = expand(&scatter(80, 5.0), &steep, [0.0, 0.0]);
        assert!(strict.len() < permissive.len());
    }

    #[test]
    fn clustering_gathers_trees_into_clumps() {
        // With clump centres, placed trees sit near a centre; the mean nearest-centre
        // distance is far smaller than for uniform placement over the same patch.
        let t = terrain([0.0, 0.0]);
        let mut s = scatter(120, 1.0);
        s.min_spacing_m = 0.3;
        let uniform = expand(&s, &t, [0.0, 0.0]);
        s.clusters = 5;
        s.cluster_radius_m = 4.0;
        let clumped = expand(&s, &t, [0.0, 0.0]);
        let half = t.half_m();
        // Re-derive the same centres the clustered run used (same seed/stream order).
        let mut stream = EntropyApi::stream(
            s.seed,
            &SpaceApi::child(&SpaceApi::root(), SCATTER_SEGMENT),
            SCATTER_VERSION,
        );
        let centers: Vec<(f32, f32)> = (0..s.clusters)
            .map(|_| (lerp(-half, half, unit(&mut stream)), lerp(-half, half, unit(&mut stream))))
            .collect();
        let mean_to_centre = |trees: &[Tree]| -> f32 {
            let sum: f32 = trees
                .iter()
                .map(|tr| {
                    centers
                        .iter()
                        .map(|&(cx, cz)| (tr.x - cx).powi(2) + (tr.z - cz).powi(2))
                        .fold(f32::MAX, f32::min)
                        .sqrt()
                })
                .sum();
            sum / trees.len() as f32
        };
        assert!(!clumped.is_empty() && !uniform.is_empty());
        assert!(mean_to_centre(&clumped) < mean_to_centre(&uniform), "clustered trees hug centres");
    }

    #[test]
    fn keep_clear_radius_excludes_trunks_near_the_camera() {
        // With a keep-clear radius around the camera's ground position, no placed
        // tree falls inside that radius — the near-camera area stays open.
        let t = terrain([0.0, 0.0]);
        let mut s = scatter(120, 1.0);
        s.keep_clear_m = 8.0;
        let center = [5.0, -4.0];
        let trees = expand(&s, &t, center);
        assert!(!trees.is_empty());
        let clear_sq = s.keep_clear_m * s.keep_clear_m;
        for tree in &trees {
            let d = (tree.x - center[0]).powi(2) + (tree.z - center[1]).powi(2);
            assert!(d >= clear_sq, "a trunk landed inside the keep-clear radius");
        }
        // With no keep-clear (default 0), trees are free to land near the centre.
        let open = expand(&scatter(120, 1.0), &t, center);
        assert!(open.iter().any(|tree| {
            (tree.x - center[0]).powi(2) + (tree.z - center[1]).powi(2) < clear_sq
        }));
    }
}
