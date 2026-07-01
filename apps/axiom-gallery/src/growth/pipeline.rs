//! Data-driven worldgen pipeline: ordered stages resolved from a list of stage
//! ids against a registry. Stage order is data, stages are keyed by stable id,
//! and an unknown stage id fails loudly.
//!
//! The pipeline runner and the `Stage` contract live here; concrete stage
//! implementations live in `stages/` and register themselves into a
//! [`StageRegistry`]. The default order mirrors Growth's `default_globe`.

use std::collections::HashMap;

use axiom_entropy::{EntropyApi, EntropyStream};
use axiom_space::{Address, SpaceApi};

use crate::growth::model_planet::PlanetGlobe;

/// Opaque, fixed address segment naming the growth worldgen root site. The value
/// is arbitrary but *stable* — a depth-1 child of the space root — so the entropy
/// key derived from `(seed, address, version)` is reproducible across runs and
/// platforms. Do not change it without accepting a full re-baseline of every
/// generated world.
const WORLDGEN_ROOT_SEGMENT: u64 = 0x_67_72_6F_77_74_68_00_01; // "growth\0\x01"
/// Generator version for the worldgen entropy key. Bumping it re-keys every
/// stream (a deliberate, versioned worldgen behavior change).
const WORLDGEN_VERSION: u32 = 1;

/// The deterministic worldgen root [`EntropyStream`] for a `u64` seed. Every
/// generation subsystem mints this once and [`EntropyStream::fork`]s an isolated
/// sub-stream (by a per-purpose salt) off it, so subsystems never share a
/// sequence yet the whole world stays reproducible from the seed. Replaces the
/// deleted app-local `rng::Rng::seeded(seed)` with the engine's `axiom-entropy`
/// keying.
pub fn worldgen_stream(seed: u64) -> EntropyStream {
    let address: Address = SpaceApi::child(&SpaceApi::root(), WORLDGEN_ROOT_SEGMENT);
    EntropyApi::stream(seed, &address, WORLDGEN_VERSION)
}

/// Per-generation context handed to every stage. Plain config + a deterministic
/// seed so stages can mint the worldgen entropy stream ([`worldgen_stream`]) and
/// `fork` their own sub-streams.
#[derive(Debug, Clone)]
pub struct GenContext {
    pub seed: u64,
    /// Target land fraction in `[0,1]`.
    pub land_target: f32,
    pub planet_radius_m: f32,
    /// Requested region count target (quantised to a subdivision level).
    pub site_target: u32,
    /// Number of tectonic plate seeds.
    pub plate_count: u32,
    /// Erosion iterations (lower in a performance profile).
    pub erosion_iterations: u32,
    /// Stage log lines, appended as the pipeline runs.
    pub log: Vec<String>,
}

impl GenContext {
    pub fn new(seed: u64) -> Self {
        Self {
            seed,
            land_target: 0.3,
            planet_radius_m: 6_371_000.0,
            site_target: 16_384,
            plate_count: 24,
            erosion_iterations: 120,
            log: Vec::new(),
        }
    }
}

/// A single worldgen stage.
pub trait Stage {
    /// Stable id matching the data-driven stage order (e.g. `"elevation"`).
    fn id(&self) -> &'static str;
    /// Mutate the globe in place.
    fn run(&self, globe: &mut PlanetGlobe, ctx: &mut GenContext);
}

impl std::fmt::Debug for dyn Stage {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Stage({})", self.id())
    }
}

/// Error when a requested stage id has no registered implementation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MissingStage(pub String);

/// Constructs a fresh boxed stage on demand (one entry per registered id).
type StageFactory = Box<dyn Fn() -> Box<dyn Stage>>;

/// Maps stage ids to constructors so a pipeline can be built from a data list.
#[derive(Default)]
pub struct StageRegistry {
    factories: HashMap<&'static str, StageFactory>,
}

impl std::fmt::Debug for StageRegistry {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let mut ids: Vec<&&str> = self.factories.keys().collect();
        ids.sort();
        write!(f, "StageRegistry({:?})", ids)
    }
}

impl StageRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    /// Register a stage constructor under its id.
    pub fn register<F>(&mut self, id: &'static str, factory: F)
    where
        F: Fn() -> Box<dyn Stage> + 'static,
    {
        self.factories.insert(id, Box::new(factory));
    }

    pub fn contains(&self, id: &str) -> bool {
        self.factories.contains_key(id)
    }

    /// Build a pipeline from an ordered list of stage ids. Fails loudly if any
    /// id is unknown.
    pub fn build(&self, ordered_ids: &[&str]) -> Result<Pipeline, MissingStage> {
        let mut stages = Vec::with_capacity(ordered_ids.len());
        for id in ordered_ids {
            match self.factories.get(id) {
                Some(make) => stages.push(make()),
                None => return Err(MissingStage((*id).to_string())),
            }
        }
        Ok(Pipeline { stages })
    }
}

/// An ordered, runnable sequence of stages.
#[derive(Debug)]
pub struct Pipeline {
    stages: Vec<Box<dyn Stage>>,
}

impl Pipeline {
    /// Run every stage in order, mutating the globe.
    pub fn run(&self, globe: &mut PlanetGlobe, ctx: &mut GenContext) {
        for stage in &self.stages {
            ctx.log.push(format!("stage:{}", stage.id()));
            stage.run(globe, ctx);
        }
    }

    pub fn stage_ids(&self) -> Vec<&'static str> {
        self.stages.iter().map(|s| s.id()).collect()
    }
}

/// The canonical overworld stage order. Implemented stages are filled by
/// `stages/`; absent ones are registered as no-ops until implemented.
pub const DEFAULT_GLOBE: &[&str] = &[
    "topology",
    "half_edge_mesh",
    "region_neighbours",
    "tectonic_plates",
    "plate_properties",
    "elevation",
    "erosion",
    "fit_land_coverage",
    "moisture",
    // Climate must run after elevation/moisture and before triangle_values.
    "wind_field",
    "moisture_advection",
    "rain_shadow",
    "triangle_values",
    "priority_flood",
    "river_downflow",
    "river_flow",
    "river_carve",
];
