//! [`PlanetGenApi`] — the deterministic planet-generation facade.
//!
//! [`PlanetGenApi::generate`] composes the `geosphere` topology and the thirteen
//! worldgen stages, in the fixed pipeline order, as a **straight-line branchless
//! call sequence** — no stage registry, no `Box<dyn Fn>`, no runner loop — into a
//! durable [`PlanetSurfaceAtlas`]. [`PlanetGenApi::sample`] /
//! [`PlanetGenApi::locate`] / [`PlanetGenApi::sample_region`] answer deterministic
//! surface queries over it.

use axiom_entropy::{EntropyApi, EntropyStream};
use axiom_geosphere::{build_icosphere, build_region_graph, subdivisions_for_target, RegionId};
use axiom_math::Vec3;
use axiom_space::{Address, SpaceApi};

use crate::atlas::build_atlas;
use crate::globe::PlanetGlobe;
use crate::ids::{PlanetGenParams, PlanetSurfaceAtlas, SurfaceSample};
use crate::{query, stages};

/// Opaque, fixed address segment naming the worldgen root site — a depth-1 child
/// of the space root, so the entropy key derived from `(seed, address, version)`
/// is reproducible across runs and platforms. Do not change it without accepting
/// a full re-baseline of every generated world.
const WORLDGEN_ROOT_SEGMENT: u64 = 0x_67_72_6F_77_74_68_00_01; // "growth\0\x01"
/// Generator version for the worldgen entropy key. Bumping it re-keys every stream.
const WORLDGEN_VERSION: u32 = 1;

/// The deterministic worldgen root [`EntropyStream`] for a `u64` seed. Every stage
/// forks an isolated sub-stream (by a per-purpose salt) off it, so stages never
/// share a sequence yet the whole planet stays reproducible from the seed.
pub(crate) fn worldgen_stream(seed: u64) -> EntropyStream {
    let address: Address = SpaceApi::child(&SpaceApi::root(), WORLDGEN_ROOT_SEGMENT);
    EntropyApi::stream(seed, &address, WORLDGEN_VERSION)
}

/// The deterministic planet-generation facade.
#[derive(Debug)]
pub struct PlanetGenApi;

impl PlanetGenApi {
    /// Generate a planet from [`PlanetGenParams`]: build the icosphere topology
    /// and region graph, run the thirteen worldgen stages in the fixed pipeline
    /// order as direct calls, and return the durable [`PlanetSurfaceAtlas`].
    /// Deterministic in the params (same params → byte-identical atlas).
    pub fn generate(params: PlanetGenParams) -> PlanetSurfaceAtlas {
        let root = worldgen_stream(params.seed);
        let topology = build_icosphere(subdivisions_for_target(params.site_target));
        let graph = build_region_graph(&topology);
        let mut globe = PlanetGlobe {
            topology,
            graph,
            ..PlanetGlobe::default()
        };
        globe.resize_fields();

        stages::tectonic_plates(&mut globe, params.plate_count, &root);
        stages::plate_properties(&mut globe, &root);
        stages::elevation(&mut globe, params.seed);
        stages::erosion(&mut globe, params.erosion_iters);
        stages::fit_land_coverage(&mut globe, params.land_target.get());
        stages::moisture(&mut globe);
        stages::wind_field(&mut globe);
        stages::moisture_advection(&mut globe);
        stages::rain_shadow(&mut globe);
        stages::triangle_values(&mut globe);
        stages::priority_flood(&mut globe);
        stages::river_downflow(&mut globe);
        stages::river_flow(&mut globe);
        stages::river_carve(&mut globe);

        build_atlas(&globe, params.radius_m)
    }

    /// Sample overworld fields at a unit direction (query-time temperature/biome).
    pub fn sample(atlas: &PlanetSurfaceAtlas, dir: Vec3) -> SurfaceSample {
        query::sample(atlas, dir)
    }

    /// The region whose site direction is closest to `dir` (max dot).
    pub fn locate(atlas: &PlanetSurfaceAtlas, dir: Vec3) -> RegionId {
        query::locate(atlas, dir)
    }

    /// Sample overworld fields directly by region id (out-of-range → default).
    pub fn sample_region(atlas: &PlanetSurfaceAtlas, region: RegionId) -> SurfaceSample {
        query::sample_region(atlas, region)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axiom_kernel::{Meters, Ratio};

    fn params(seed: u64) -> PlanetGenParams {
        PlanetGenParams {
            seed,
            radius_m: Meters::finite_or_zero(6_371_000.0),
            land_target: Ratio::finite_or_zero(0.3),
            site_target: 162, // subdivision 2 → 162 regions
            plate_count: 12,
            erosion_iters: 24,
        }
    }

    #[test]
    fn generate_builds_a_populated_atlas() {
        let atlas = PlanetGenApi::generate(params(0xA11CE));
        assert_eq!(atlas.region_count(), 162);
        assert_eq!(atlas.planet_radius_m.get(), 6_371_000.0);
        assert_eq!(atlas.region_elevation.len(), 162);
        assert_eq!(atlas.region_moisture.len(), 162);
        assert!(atlas.locator.bands > 0);
        // fit_land_coverage aimed at ~30% land: both land and ocean exist.
        assert!(atlas.region_elevation.iter().any(|&e| e >= 0.0));
        assert!(atlas.region_elevation.iter().any(|&e| e < 0.0));
    }

    #[test]
    fn generation_is_deterministic() {
        let a = PlanetGenApi::generate(params(42));
        let b = PlanetGenApi::generate(params(42));
        assert_eq!(a.region_elevation, b.region_elevation);
        assert_eq!(a.region_moisture, b.region_moisture);
        assert_eq!(a.region_plate, b.region_plate);
        assert_eq!(a.plate_oceanic, b.plate_oceanic);
    }

    #[test]
    fn distinct_seeds_produce_distinct_planets() {
        let a = PlanetGenApi::generate(params(1));
        let b = PlanetGenApi::generate(params(2));
        assert_ne!(a.region_elevation, b.region_elevation);
    }

    #[test]
    fn queries_over_the_generated_atlas() {
        let atlas = PlanetGenApi::generate(params(7));
        // Every region site is nearest to itself.
        (0..atlas.region_count()).for_each(|i| {
            let dir = atlas.sites[i];
            assert_eq!(PlanetGenApi::locate(&atlas, dir), RegionId(i as u32));
        });
        let dir = atlas.sites[10];
        let s = PlanetGenApi::sample(&atlas, dir);
        assert_eq!(s.region, RegionId(10));
        assert_eq!(
            PlanetGenApi::sample_region(&atlas, RegionId(10)).region,
            RegionId(10)
        );
    }

    #[test]
    fn worldgen_stream_is_reproducible() {
        assert_eq!(worldgen_stream(5).key(), worldgen_stream(5).key());
        assert_ne!(worldgen_stream(5).key(), worldgen_stream(6).key());
    }

    #[test]
    fn facade_is_debug() {
        assert!(!format!("{:?}", PlanetGenApi).is_empty());
    }
}
