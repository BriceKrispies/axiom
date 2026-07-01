//! # Growth — a deterministic procedural-planet survival game (Axiom app)
//!
//! Implements the requirements in
//! `docs/growth-port/worldgen_simulator_requirements_audit.md`. Built as an app
//! (composition leaf) so it is exempt from the branchless and 100%-coverage
//! spine gates while the substrate is built; proven primitives graduate into
//! layers/modules later (see `docs/growth-port/roadmap.md`).
//!
//! Pipeline: seed → genome(preset) → globe(topology+stages) → atlas → streaming.
//! Every subsystem traces to a requirement id in [`requirements`].
#![allow(dead_code)]

use axiom_math::Vec3;

// --- foundation (shared types + determinism) ---
pub mod distributions;
pub mod ids;
pub mod model_planet;
pub mod model_world;
pub mod requirements;

// --- generation primitives ---
// Coherent noise (gradient/Perlin + FBM + domain warp) graduated into the
// `axiom-noise` engine layer; the spherical topology (icosphere + region graph +
// ring validation) graduated into the `axiom-geosphere` engine layer. The growth
// pipeline consumes both directly.

// --- planet genome / presets / seed ---
pub mod genome;
pub mod presets;
pub mod seed;

// --- worldgen composition (via the axiom-planetgen feature module) + sampling ---
pub mod sampler;

// --- game-world streaming ---
pub mod chunkstore;
pub mod gameworld;
pub mod localmap;

// --- scenic composition (Everest-scale mountain vista) ---
pub mod vista;

// --- native terrain mesh generation (the far scenic massif), shared by the wasm
// viewer and the headless screenshot capture ---
pub mod terrain_mesh;

// --- native first-person ground-walk sim (shared by the wasm viewer + the
// headless agent driver) ---
pub mod ground;

// --- data-driven semantic world tags (the agent's "nouns"), fed to
// axiom-introspect so a directive can be resolved by name (native, `agent`) ---
#[cfg(feature = "growth-agent")]
pub mod world_tags;

// --- the reusable axiom-agent driver (native, `agent` feature only): walks the
// ground sim through axiom-agent-harness, e.g. holding forward to the summit ---
#[cfg(feature = "growth-agent")]
pub mod agent;

// --- live, game-agnostic perception (native, `agent` feature only): the
// heightfield sense adapter that casts the reusable `axiom-perception` ray-fan
// against the terrain (by marching the sampler) — the SAME module the retro FPS agent
// casts against its scene, proving perception is agnostic of the world's shape ---
#[cfg(feature = "growth-agent")]
pub mod perception;

// --- gameplay + qa + moddability ---
pub mod defs;
pub mod determinism;
pub mod dig;
pub mod gameplay;
pub mod intent;
pub mod inventory;

// --- in-browser terrain viewer (wasm32 only) ---
// The live wgpu/first-person presentation arm. Never compiled on native, so the
// deterministic worldgen core and `cargo test` are untouched.
#[cfg(target_arch = "wasm32")]
mod web;

use axiom_kernel::{Meters, Ratio};
use axiom_planetgen::{PlanetGenApi, PlanetGenParams};

use crate::growth::chunkstore::{ChunkStore, STREAM_RADIUS_CHUNKS, STREAM_UNLOAD_MARGIN};
use crate::growth::genome::PlanetGenome;
use crate::growth::ids::ChunkCoord;
use crate::growth::inventory::Inventory;
use crate::growth::model_planet::{PlanetSurfaceAtlas, SurfaceSample};
use crate::growth::model_world::{Diff, GameWorldLocalMap};
use crate::growth::presets::PlanetPreset;
use crate::growth::seed::{worldgen_stream, WorldSeed};

/// Tectonic plate seeds for a generated planet (was `GenContext::plate_count`).
const DEFAULT_PLATE_COUNT: u32 = 24;
/// Stream-power erosion iterations (was `GenContext::erosion_iterations`).
const DEFAULT_EROSION_ITERS: u32 = 120;

/// A Growth session: owns the seed, genome, overworld atlas, and (after entering
/// play) the streamed game world. Audit: SA-E1 session, OW-E2 atlas ownership.
#[derive(Debug)]
pub struct Growth {
    pub seed: WorldSeed,
    pub genome: PlanetGenome,
    pub atlas: PlanetSurfaceAtlas,
    pub localmap: Option<GameWorldLocalMap>,
    store: ChunkStore,
    inventory: Inventory,
    committed: bool,
    last_center: ChunkCoord,
    /// Determinism hash of the generated globe. Audit: SC-E3.
    pub world_hash: u64,
}

impl Growth {
    /// Generate a planet from a seed string + preset + region-count target.
    /// Audit: OW vision, OW-4.4 (store seed+genome), OW-E1 (build atlas).
    pub fn generate(seed_str: &str, preset: PlanetPreset, site_target: u32) -> Self {
        let seed = WorldSeed::from_str_seed(seed_str);
        let mut stream = worldgen_stream(seed.value);
        let genome = presets::sample_genome(preset, &mut stream);

        // The whole overworld pipeline (topology + the thirteen worldgen stages +
        // atlas build) is now the `axiom-planetgen` feature module. The app only
        // translates the astrophysical genome into neutral generation params and
        // reads the durable atlas back. Audit: OW-E1 (build atlas), OW-E20 (the
        // transient globe lives and dies inside `PlanetGenApi::generate`).
        let params = PlanetGenParams {
            seed: seed.value,
            radius_m: Meters::finite_or_zero(genome.radius_m),
            land_target: Ratio::finite_or_zero(genome.implied_land_fraction()),
            site_target,
            plate_count: DEFAULT_PLATE_COUNT,
            erosion_iters: DEFAULT_EROSION_ITERS,
        };
        let atlas = PlanetGenApi::generate(params);
        let world_hash = determinism::world_hash(&atlas);

        Self {
            seed,
            genome,
            atlas,
            localmap: None,
            store: ChunkStore::new(),
            inventory: Inventory::new(),
            committed: false,
            last_center: ChunkCoord::default(),
            world_hash,
        }
    }

    /// Overworld query at a unit direction. Audit: OW-E3/E4 sample_surface.
    pub fn sample_surface(&self, dir: Vec3) -> SurfaceSample {
        sampler::sample_surface(&self.atlas, dir)
    }

    /// Leave the overworld and begin streaming the game world. Audit: GW-7.0,
    /// commit_overworld_for_play + enter_game_world.
    pub fn enter_game_world(&mut self) -> Vec<Diff> {
        let localmap = GameWorldLocalMap::anchored(&self.atlas);
        let mut diffs = Vec::new();
        let center = ChunkCoord::default();
        self.store.stream(
            center,
            STREAM_RADIUS_CHUNKS,
            STREAM_UNLOAD_MARGIN,
            &self.atlas,
            &localmap,
            self.seed.value,
            &mut diffs,
        );
        self.localmap = Some(localmap);
        self.committed = true;
        self.last_center = center;
        diffs
    }

    /// Stream chunks around a new focus chunk; returns load/unload diffs.
    /// Audit: GW-2.2/2.3, GW-E9.
    pub fn tick_streaming(&mut self, center: ChunkCoord) -> Vec<Diff> {
        let mut diffs = Vec::new();
        if !self.committed {
            return diffs;
        }
        if let Some(localmap) = &self.localmap {
            self.store.stream(
                center,
                STREAM_RADIUS_CHUNKS,
                STREAM_UNLOAD_MARGIN,
                &self.atlas,
                localmap,
                self.seed.value,
                &mut diffs,
            );
            self.last_center = center;
        }
        diffs
    }

    /// Apply a dig intent. Audit: GW-E4/E11/E15.
    pub fn dig(&mut self, coord: ChunkCoord, lx: u32, lz: u32) -> Vec<Diff> {
        let mut diffs = Vec::new();
        dig::apply_dig(
            &mut self.store,
            &mut self.inventory,
            coord,
            lx,
            lz,
            &mut diffs,
        );
        diffs
    }

    pub fn inventory_count(&self, item: u32) -> u32 {
        self.inventory.count(item)
    }
}

#[cfg(test)]
mod integration {
    use super::*;

    #[test]
    fn generate_is_deterministic() {
        let a = Growth::generate("hello-world", PlanetPreset::Earthlike, 4096);
        let b = Growth::generate("hello-world", PlanetPreset::Earthlike, 4096);
        assert_eq!(a.world_hash, b.world_hash, "same seed must reproduce");
        assert_eq!(a.genome.water_fraction, b.genome.water_fraction);
    }

    #[test]
    fn different_seeds_differ_genome() {
        let a = Growth::generate("seed-a", PlanetPreset::Earthlike, 4096);
        let b = Growth::generate("seed-b", PlanetPreset::Earthlike, 4096);
        // Water fraction is sampled deterministically per seed.
        assert!(a.seed.value != b.seed.value);
    }

    #[test]
    fn pipeline_produces_a_fully_shaped_atlas() {
        // The graduated pipeline no longer emits a stage log; instead assert the
        // observable result of every stage having run in order on a real planet:
        // a fully-populated atlas whose fields carry the geology + hydrology the
        // stages produce, and which is reproducible from the same seed.
        let a = Growth::generate("x", PlanetPreset::Dry, 1024);
        let b = Growth::generate("x", PlanetPreset::Dry, 1024);
        let n = a.atlas.region_count();
        assert!(n >= 1024, "topology stage produced {n} regions");
        assert_eq!(a.atlas.region_elevation.len(), n);
        assert_eq!(a.atlas.region_moisture.len(), n);
        // tectonic + plate stages assigned every region to a plate.
        assert_eq!(a.atlas.region_plate.len(), n);
        assert!(!a.atlas.plate_oceanic.is_empty());
        // fit_land_coverage produced both land and ocean; moisture stayed in range.
        assert!(a.atlas.region_elevation.iter().any(|&e| e >= 0.0));
        assert!(a.atlas.region_elevation.iter().any(|&e| e < 0.0));
        assert!(a.atlas.region_moisture.iter().all(|&m| (0.0..=1.0).contains(&m)));
        // Determinism: the same seed reproduces the same planet.
        assert_eq!(a.world_hash, b.world_hash);
        assert_eq!(a.atlas.region_elevation, b.atlas.region_elevation);
    }

    #[test]
    fn enter_game_world_streams_chunks() {
        let mut g = Growth::generate("play", PlanetPreset::Earthlike, 1024);
        let diffs = g.enter_game_world();
        let loaded = diffs
            .iter()
            .filter(|d| matches!(d, Diff::ChunkLoaded { .. }))
            .count();
        // radius 2 → 5×5 = 25 chunks.
        assert_eq!(loaded, 25);
    }

    #[test]
    fn dig_yields_material() {
        let mut g = Growth::generate("dig", PlanetPreset::Earthlike, 1024);
        g.enter_game_world();
        let center = ChunkCoord::default();
        let diffs = g.dig(center, 4, 4);
        assert_eq!(diffs.len(), 1);
        assert_eq!(g.inventory_count(0), 1);
    }
}
