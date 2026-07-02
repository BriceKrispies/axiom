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

use super::scene::{Scatter, Terrain, Tree};

/// Fixed address segment keying the scatter entropy stream ("vtscat\0\x01"), so the
/// stream derived from `(scatter.seed, address, version)` is reproducible.
const SCATTER_SEGMENT: u64 = 0x_76_74_73_63_61_74_00_01;
const SCATTER_VERSION: u32 = 1;
/// How many rejected candidates to tolerate before giving up on reaching `count`
/// (keeps a dense/steep scene from looping forever). A generous multiple of `count`.
const MAX_ATTEMPTS_PER_TREE: u32 = 32;

/// Expand `scatter` over `terrain` into concrete [`Tree`] instances.
pub fn expand(scatter: &Scatter, terrain: &Terrain) -> Vec<Tree> {
    let address: Address = SpaceApi::child(&SpaceApi::root(), SCATTER_SEGMENT);
    let mut stream = EntropyApi::stream(scatter.seed, &address, SCATTER_VERSION);

    let half = terrain.half_m();
    let min_sq = scatter.min_spacing_m * scatter.min_spacing_m;
    let mut placed: Vec<Tree> = Vec::with_capacity(scatter.count as usize);
    let attempt_cap = scatter.count.saturating_mul(MAX_ATTEMPTS_PER_TREE);
    let mut attempts = 0u32;

    while (placed.len() as u32) < scatter.count && attempts < attempt_cap {
        attempts += 1;
        let x = lerp(-half, half, unit(&mut stream));
        let z = lerp(-half, half, unit(&mut stream));

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

/// A uniform `[0, 1)` sample as `f32`, off the deterministic stream.
fn unit(stream: &mut EntropyStream) -> f32 {
    stream.unit().get()
}

/// A uniform sample in `[lo, hi]` from a `[min, max]` range.
fn range(stream: &mut EntropyStream, bounds: [f32; 2]) -> f32 {
    lerp(bounds[0], bounds[1], unit(stream))
}

fn lerp(a: f32, b: f32, t: f32) -> f32 {
    a + (b - a) * t
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::growth::visual_target::scene::Octave;

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
            trunk_height_m: [4.0, 8.0],
            trunk_radius_m: [0.2, 0.4],
            canopy_radius_m: [2.0, 4.0],
            canopy_palette: vec![[0.8, 0.4, 0.1], [0.86, 0.62, 0.18]],
        }
    }

    #[test]
    fn same_seed_reproduces_the_forest() {
        let t = terrain([0.02, 0.02]);
        let a = expand(&scatter(60, 1.0), &t);
        let b = expand(&scatter(60, 1.0), &t);
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
        let trees = expand(&s, &t);
        let min_sq = s.min_spacing_m * s.min_spacing_m;
        for (i, a) in trees.iter().enumerate() {
            for b in &trees[i + 1..] {
                let d = (a.x - b.x).powi(2) + (a.z - b.z).powi(2);
                assert!(d >= min_sq, "two trees closer than min spacing");
            }
        }
    }

    #[test]
    fn steep_slope_limit_places_fewer_trees() {
        // A steep linear ramp with a strict slope limit rejects most sites.
        let steep = terrain([0.9, 0.9]);
        let strict = expand(&scatter(80, 0.2), &steep);
        let permissive = expand(&scatter(80, 5.0), &steep);
        assert!(strict.len() < permissive.len());
    }
}
