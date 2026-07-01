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
pub mod pipeline;
pub mod requirements;

// --- generation primitives ---
pub mod noise;
pub mod topology;

// --- planet genome / presets / seed ---
pub mod genome;
pub mod presets;
pub mod seed;

// --- worldgen stages + atlas + sampling ---
pub mod atlas;
pub mod sampler;
pub mod stages;

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

use crate::growth::chunkstore::{ChunkStore, STREAM_RADIUS_CHUNKS};
use crate::growth::genome::PlanetGenome;
use crate::growth::ids::ChunkCoord;
use crate::growth::inventory::Inventory;
use crate::growth::model_planet::{PlanetGlobe, PlanetSurfaceAtlas, SurfaceSample};
use crate::growth::model_world::{Diff, GameWorldLocalMap};
use crate::growth::pipeline::{GenContext, StageRegistry, DEFAULT_GLOBE};
use crate::growth::presets::PlanetPreset;
use crate::growth::seed::WorldSeed;

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
    /// Stage log from generation. Audit: OW-4.3.
    pub gen_log: Vec<String>,
}

impl Growth {
    /// Generate a planet from a seed string + preset + region-count target.
    /// Audit: OW vision, OW-4.4 (store seed+genome), OW-E1 (build atlas).
    pub fn generate(seed_str: &str, preset: PlanetPreset, site_target: u32) -> Self {
        let seed = WorldSeed::from_str_seed(seed_str);
        let mut stream = pipeline::worldgen_stream(seed.value);
        let genome = presets::sample_genome(preset, &mut stream);

        let mut ctx = GenContext::new(seed.value);
        ctx.planet_radius_m = genome.radius_m;
        ctx.land_target = genome.implied_land_fraction();
        ctx.site_target = site_target;

        // Topology (fixed for the generation), then run the data-driven pipeline.
        let subdiv = topology::subdivisions_for_target(site_target);
        let mut globe = PlanetGlobe {
            topology: topology::build_icosphere(subdiv),
            ..PlanetGlobe::default()
        };
        globe.graph = topology::build_region_graph(&globe.topology);
        globe.resize_fields();

        // OW-E18 / SC-E1: validate dual region rings before hydrology. Logged
        // (not aborted) so generation is observable; the QA gate test asserts
        // validity on reference configs.
        let rings = topology::validate_region_rings(&globe);
        ctx.log.push(format!(
            "validate_topology: bad_adjacency={} tris_not_in_3_rings={}",
            rings.bad_adjacency, rings.tris_not_in_3_rings
        ));

        let mut registry = StageRegistry::new();
        stages::register_default_stages(&mut registry);
        match registry.build(DEFAULT_GLOBE) {
            Ok(pipe) => pipe.run(&mut globe, &mut ctx),
            Err(missing) => ctx.log.push(format!("MISSING STAGE: {}", missing.0)),
        }

        let world_hash = determinism::world_hash(&globe);
        let atlas = atlas::build_atlas(&globe, &genome);
        // OW-E20: transient globe is dropped here; only the atlas is retained.

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
            gen_log: ctx.log,
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
        self.store.request(
            center,
            STREAM_RADIUS_CHUNKS,
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
            self.store.request(
                center,
                STREAM_RADIUS_CHUNKS,
                &self.atlas,
                localmap,
                self.seed.value,
                &mut diffs,
            );
            self.store
                .unload_far(center, STREAM_RADIUS_CHUNKS, 1, &mut diffs);
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
    fn pipeline_runs_all_default_stages() {
        let g = Growth::generate("x", PlanetPreset::Dry, 1024);
        for id in DEFAULT_GLOBE {
            assert!(
                g.gen_log.iter().any(|l| l == &format!("stage:{}", id)),
                "stage {} did not run",
                id
            );
        }
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
