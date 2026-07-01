//! Overworld surface queries — a thin app adapter over the graduated
//! `axiom-planetgen` feature module.
//!
//! `locate_region` / `sample_surface` / `sample_region` delegate to
//! [`PlanetGenApi`], which owns the branchless nearest-site locator and the
//! climate-lens composition. The `biome` colour-code vocabulary (the `CLIMATE_*`
//! codes the browser viewer and the terrain mesher key their palettes on) stays
//! here, mirroring `axiom_biome`'s climate constants.

use axiom_geosphere::RegionId;
use axiom_math::Vec3;
use axiom_planetgen::PlanetGenApi;

use crate::growth::model_planet::{PlanetSurfaceAtlas, SurfaceSample};

/// Find the region whose site direction is closest to `dir`. Audit: OW-E3.
pub fn locate_region(atlas: &PlanetSurfaceAtlas, dir: Vec3) -> RegionId {
    PlanetGenApi::locate(atlas, dir)
}

/// Sample overworld fields at a unit direction. Audit: OW-E3/E4.
pub fn sample_surface(atlas: &PlanetSurfaceAtlas, dir: Vec3) -> SurfaceSample {
    PlanetGenApi::sample(atlas, dir)
}

/// Sample overworld fields directly by region id. Audit: OW-E3, GW-E2.
pub fn sample_region(atlas: &PlanetSurfaceAtlas, region: RegionId) -> SurfaceSample {
    PlanetGenApi::sample_region(atlas, region)
}

/// App-facing biome colour codes: the biome module's climate `CLIMATE_*`
/// vocabulary as the `u32` codes the atlas/rendering key their colours on
/// (ocean when below sea level 0; otherwise hot/cold × wet/dry).
pub mod biome {
    use axiom_biome::BiomeApi;
    pub const OCEAN: u32 = BiomeApi::CLIMATE_OCEAN as u32;
    pub const DESERT: u32 = BiomeApi::CLIMATE_DESERT as u32; // hot + dry
    pub const RAINFOREST: u32 = BiomeApi::CLIMATE_RAINFOREST as u32; // hot + wet
    pub const TUNDRA: u32 = BiomeApi::CLIMATE_TUNDRA as u32; // cold + dry
    pub const TAIGA: u32 = BiomeApi::CLIMATE_TAIGA as u32; // cold + wet
}
